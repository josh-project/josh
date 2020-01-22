#[macro_use]
extern crate josh;

extern crate clap;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate rand;
extern crate regex;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_json;

extern crate crypto;
extern crate tokio_core;
extern crate tracing;
extern crate tracing_log;
extern crate tracing_subscriber;

use futures::future::Future;
use futures::Stream;
use futures_cpupool::CpuPool;
use hyper::header::{Authorization, Basic};
use hyper::server::{Http, Request, Response, Service};
use josh::base_repo;
use josh::cgi;
use josh::scratch;
use josh::shell;
use josh::view_maps;
use josh::virtual_repo;
use rand::random;
use regex::Regex;
use std::env;
use std::process::exit;

use crypto::digest::Digest;
use std::collections::{HashMap, HashSet};
use std::fs::remove_dir_all;
use std::net;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, RwLock};

use tracing::{debug, span, Level};

use tracing::*;
use tracing_subscriber::layer::Layer;

lazy_static! {
    static ref VIEW_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<headref>@[^:!]*)?(?P<view>[:!].*)[.](?P<ending>(?:git)|(?:json))(?P<pathinfo>/.*)?")
            .expect("can't compile regex");
}

type CredentialCache = HashMap<String, std::time::Instant>;
type KnownViews = HashMap<String, HashSet<String>>;

struct HttpService {
    handle: tokio_core::reactor::Handle,
    fetch_push_pool: CpuPool,
    housekeeping_pool: CpuPool,
    compute_pool: CpuPool,
    port: String,
    base_path: PathBuf,
    base_url: String,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    known_views: Arc<RwLock<KnownViews>>,
    fetching: Arc<RwLock<HashSet<String>>>,
}

fn hash_strings(url: &str, username: &str, password: &str) -> String {
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

fn fetch_upstream(
    http: &HttpService,
    prefix: &str,
    username: &str,
    password: &str,
    remote_url: String,
    headref: &str,
) -> Box<futures_cpupool::CpuFuture<std::result::Result<(), git2::Error>, hyper::Error>> {
    let credentials_hashed = hash_strings(&remote_url, &username, &password);
    let username = username.to_owned();
    let password = password.to_owned();
    let prefix = prefix.to_owned();
    let br_path = http.base_path.clone();
    let credential_cache = http.credential_cache.clone();
    let fetching = http.fetching.clone();
    let headref = headref.to_owned();

    let credentials_cached_ok = headref == "" && {
        let last = http
            .credential_cache
            .read()
            .ok()
            .map(|cc| cc.get(&credentials_hashed).copied());

        if let Some(Some(c)) = last {
            std::time::Instant::now().duration_since(c) < std::time::Duration::from_secs(60)
        } else {
            false
        }
    };

    let do_fetch = if credentials_cached_ok
        && !fetching.write().unwrap().insert(credentials_hashed.clone())
    {
        Box::new(
            http.compute_pool
                .spawn(futures::future::ok(()).map(move |_| Ok(()))),
        )
    } else {
        Box::new(
            http.fetch_push_pool
                .spawn(futures::future::ok(()).map(move |_| {
                    let refs_to_fetch = if headref != "" {
                        vec![headref.as_str()]
                    } else {
                        vec!["refs/heads/*", "refs/tags/*"]
                    };
                    let credentials_hashed = hash_strings(&remote_url, &username, &password);
                    debug!("credentials_hashed {:?}, {:?}, {:?}", &remote_url, &username, &credentials_hashed);
                    base_repo::fetch_refs_from_url(
                        &br_path,
                        &prefix,
                        &remote_url,
                        &refs_to_fetch,
                        &username,
                        &password,
                    )
                    .and_then(|_| {
                        fetching.write().unwrap().remove(&credentials_hashed);
                        if let Ok(mut cc) = credential_cache.write() {
                            cc.insert(credentials_hashed, std::time::Instant::now());
                        }
                        Ok(())
                    })
                })),
        )
    };

    if credentials_cached_ok {
        do_fetch.forget();
        return Box::new(http.compute_pool.spawn(futures::future::ok(Ok(()))));
    }

    return do_fetch;
}

fn async_fetch(
    http: &HttpService,
    prefix: &str,
    headref: &str,
    view_string: &str,
    username: &str,
    password: &str,
    namespace: &str,
    remote_url: String,
) -> Box<dyn Future<Item = Result<PathBuf, git2::Error>, Error = hyper::Error>> {
    let br_path = http.base_path.clone();
    base_repo::create_local(&br_path);

    let fetch_future = fetch_upstream(http, prefix, username, password, remote_url, headref);

    let headref = headref.to_owned();
    let viewstr = view_string.to_owned();
    let forward_maps = http.forward_maps.clone();
    let backward_maps = http.backward_maps.clone();
    let namespace = namespace.to_owned();
    let br_path = http.base_path.clone();
    let prefix = prefix.to_owned();
    let viewstr2 = view_string.to_owned();
    let forward_maps2 = http.forward_maps.clone();
    let backward_maps2 = http.backward_maps.clone();
    let br_path2 = http.base_path.clone();
    let prefix2 = prefix.to_owned();
    let cp = http.compute_pool.clone();
    let known_views = http.known_views.clone();
    Box::new(http.compute_pool.spawn(fetch_future.map(move |r| {
        let refresh_all_known_views = cp.spawn_fn(move || -> Result<(), ()> {
            discover_views("master", &br_path2, known_views.clone());
            if let Ok(mut kn) = known_views.try_write() {
                kn.entry(prefix2.clone())
                    .or_insert_with(HashSet::new)
                    .insert(viewstr2);
            } else {
                // If we could not get write lock that means a rebuild is in progress,
                // So don't trigger another one.
                return Ok(());
            }
            if let Ok(kn) = known_views.read() {
                let _trace_s = span!(Level::TRACE, "refresh_all_known_views");
                if let Some(e) = kn.get(&prefix2) {
                    for v in e.iter() {
                        make_view_repo(
                            &v,
                            &prefix2,
                            "HEAD",
                            &hash_strings(&prefix2, &v, ""),
                            &br_path2,
                            forward_maps2.clone(),
                            backward_maps2.clone(),
                        );
                    }
                }
            }
            Ok(())
        });
        refresh_all_known_views.forget();
        r.map(move |_| {
            make_view_repo(
                &viewstr,
                &prefix,
                &headref,
                &namespace,
                &br_path,
                forward_maps,
                backward_maps,
            )
        })
    })))
}

fn respond_unauthorized() -> Response {
    let mut response: Response = Response::new().with_status(hyper::StatusCode::Unauthorized);
    response
        .headers_mut()
        .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\"");
    response
}

fn parse_url(path: &str) -> Option<(String, String, String, String, String)> {
    let nop_path = path.replacen(".git", ".git:nop=nop.git", 1);
    let caps = if let Some(caps) = VIEW_REGEX.captures(&path) {
        caps
    } else {
        if let Some(caps) = VIEW_REGEX.captures(&nop_path) {
            caps
        } else {
            return None;
        }
    };

    let as_str = |x: regex::Match| x.as_str().to_owned();
    debug!("parse_url: {:?}", caps);

    return Some((
        caps.name("prefix").map(as_str).unwrap_or("".to_owned()),
        caps.name("view").map(as_str).unwrap_or("".to_owned()),
        caps.name("pathinfo").map(as_str).unwrap_or("".to_owned()),
        caps.name("headref").map(as_str).unwrap_or("".to_owned()),
        caps.name("ending").map(as_str).unwrap_or("".to_owned()),
    ));
}

fn call_service(
    service: &HttpService,
    req: Request,
    namespace: &str,
) -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if path == "/version" {
        let v = option_env!("GIT_DESCRIBE").unwrap_or(env!("CARGO_PKG_VERSION"));
        let response = Response::new()
            .with_body(format!("Version: {}\n", v))
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if path == "/reset" {
        base_repo::reset_all(&service.base_path);
        let response = Response::new()
            .with_body("deleted".to_owned())
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if path == "/gc" {
        let br_path = service.base_path.clone();
        return Box::new(service.housekeeping_pool.spawn_fn(move || {
            let response = Response::new()
                .with_body(base_repo::run_housekeeping(&br_path, "git gc"))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }
    if path == "/repack" {
        let br_path = service.base_path.clone();
        return Box::new(service.housekeeping_pool.spawn_fn(move || {
            let response = Response::new()
                .with_body(base_repo::run_housekeeping(&br_path, "git repack -Ad"))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }
    if path == "/count-objects" {
        let br_path = service.base_path.clone();
        return Box::new(service.housekeeping_pool.spawn_fn(move || {
            let response = Response::new()
                .with_body(base_repo::run_housekeeping(
                    &br_path,
                    "git count-objects -vH",
                ))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }
    if path == "/prune" {
        let br_path = service.base_path.clone();
        return Box::new(service.housekeeping_pool.spawn_fn(move || {
            let response = Response::new()
                .with_body(base_repo::run_housekeeping(&br_path, "git prune"))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }
    if path == "/fsck" {
        let br_path = service.base_path.clone();
        return Box::new(service.housekeeping_pool.spawn_fn(move || {
            let response = Response::new()
                .with_body(base_repo::run_housekeeping(&br_path, "git fsck"))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }
    if path == "/flush" {
        service.credential_cache.write().unwrap().clear();
        let response = Response::new()
            .with_body(format!("Flushed credential cache\n"))
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if path == "/views" {
        let _br_path = service.base_path.clone();
        let body = serde_json::to_string(&*service.known_views.read().unwrap()).unwrap();
        let response = Response::new()
            .with_body(body)
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if path == "/panic" {
        panic!();
    }
    if path == "/repo_update" {
        let pool = service.fetch_push_pool.clone();
        return Box::new(
            req.body()
                .concat2()
                .map(move |body| {
                    let mut buffer: Vec<u8> = Vec::new();
                    for i in body {
                        buffer.push(i);
                    }

                    String::from_utf8(buffer).unwrap_or("".to_string())
                })
                .and_then(move |buffer| {
                    return pool.spawn(futures::future::ok(buffer).map(move |buffer| {
                        let repo_update: virtual_repo::RepoUpdate = serde_json::from_str(&buffer)
                            .unwrap_or(virtual_repo::RepoUpdate::new());
                        virtual_repo::process_repo_update(repo_update, forward_maps, backward_maps)
                    }));
                })
                .and_then(move |result| {
                    if let Ok(stderr) = result {
                        let response = Response::new()
                            .with_body(stderr)
                            .with_status(hyper::StatusCode::Ok);
                        return Box::new(futures::future::ok(response));
                    }
                    if let Err(stderr) = result {
                        let response = Response::new()
                            .with_body(stderr)
                            .with_status(hyper::StatusCode::BadRequest);
                        return Box::new(futures::future::ok(response));
                    }
                    let response = Response::new().with_status(hyper::StatusCode::Forbidden);
                    return Box::new(futures::future::ok(response));
                }),
        );
    }

    let compute_pool = service.compute_pool.clone();

    let (prefix, view_string, pathinfo, headref, ending) = some_or!(parse_url(&path), {
        let response = Response::new().with_status(hyper::StatusCode::NotFound);
        return Box::new(futures::future::ok(response));
    });

    let headref = headref.trim_start_matches("@").to_owned();

    if ending == "json" {
        let forward_maps = service.forward_maps.clone();
        let backward_maps = service.forward_maps.clone();
        let br_path = service.base_path.clone();

        let f = compute_pool.spawn(futures::future::ok(true).map(move |_| {
            let info = get_info(
                &view_string,
                &prefix,
                &headref,
                &br_path,
                forward_maps.clone(),
                backward_maps.clone(),
            );
            info
        }));

        return Box::new(f.and_then(move |info| {
            let response = Response::new()
                .with_body(format!("{}\n", info))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }

    let (username, password) = match req.headers().get() {
        Some(&Authorization(Basic {
            ref username,
            ref password,
        })) => (
            username.to_owned(),
            password.to_owned().unwrap_or("".to_owned()).to_owned(),
        ),
        _ => {
            return Box::new(futures::future::ok(respond_unauthorized()));
        }
    };

    let passwd = password.clone();
    let usernm = username.clone();
    let viewstr = view_string.clone();
    let ns = namespace.to_owned();

    let port = service.port.clone();

    let remote_url = {
        let mut remote_url = service.base_url.clone();
        remote_url.push_str(&prefix);
        remote_url
    };

    let br_url = remote_url.clone();
    let base_ns = to_ns(&prefix);

    let call_git_http_backend = |request: Request,
                                 path: PathBuf,
                                 pathinfo: &str,
                                 handle: &tokio_core::reactor::Handle|
     -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
        debug!("CALLING git-http backend {:?} {:?}", path, pathinfo);
        let mut cmd = Command::new("git");
        cmd.arg("http-backend");
        cmd.current_dir(&path);
        cmd.env("GIT_PROJECT_ROOT", path.to_str().unwrap());
        cmd.env("GIT_DIR", path.to_str().unwrap());
        cmd.env("GIT_HTTP_EXPORT_ALL", "");
        cmd.env("PATH_INFO", pathinfo);
        cmd.env("JOSH_PASSWORD", passwd);
        cmd.env("JOSH_USERNAME", usernm);
        cmd.env("JOSH_PORT", port);
        cmd.env("GIT_NAMESPACE", ns);
        cmd.env("JOSH_VIEWSTR", viewstr);
        cmd.env("JOSH_REMOTE", remote_url);
        cmd.env("JOSH_BASE_NS", base_ns);

        cgi::do_cgi(request, cmd, handle.clone())
    };

    debug!("PREFIX: {}", &prefix);
    debug!("VIEW: {}", &view_string);
    debug!("PATH_INFO: {:?}", &pathinfo);

    let handle = service.handle.clone();
    let ns_path = service.base_path.clone();
    let ns_path = ns_path.join("refs/namespaces");
    let ns_path = ns_path.join(&namespace);
    assert!(namespace.contains("request_"));

    Box::new({
        async_fetch(
            &service,
            &prefix,
            &headref,
            &view_string,
            &username,
            &password,
            &namespace,
            br_url,
        )
        .and_then(
            move |view_repo| -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
                let path = ok_or!(view_repo, {
                    debug!("wrong credentials");
                    return Box::new(futures::future::ok(respond_unauthorized()));
                });

                call_git_http_backend(req, path, &pathinfo, &handle)
            },
        )
        .map(move |x| {
            if true {
                remove_dir_all(ns_path).unwrap_or_else(|e| warn!("remove_dir_all failed: {:?}", e));
            }
            x
        })
    })
}

impl Service for HttpService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let rid: usize = random();
        let rname = format!("request_{}", rid);

        let username = match req.headers().get() {
            Some(&Authorization(Basic {
                ref username,
                password: _,
            })) => username.to_owned(),
            None => "".to_owned(),
        };
        let mut headers = req.headers().clone();
        headers.set(Authorization(Basic {
            username: username,
            password: None,
        }));

        let _trace_s = span!(Level::TRACE, "call_service", path = ?req.path(), ?headers);
        Box::new(call_service(&self, req, &rname).map(move |response| {
            event!(parent: &_trace_s, Level::TRACE, ?response);
            response
        }))
    }
}

fn run_proxy(args: Vec<String>) -> i32 {
    tracing_log::LogTracer::init().expect("can't init LogTracer");
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    let filter = tracing_subscriber::filter::EnvFilter::new("josh_proxy=trace,josh=trace");
    let subscriber = filter.with_subscriber(subscriber);

    tracing::subscriber::set_global_default(subscriber).expect("failed to set");

    debug!("RUN PROXY {:?}", &args);

    let args = clap::App::new("josh-proxy")
        .arg(
            clap::Arg::with_name("remote")
                .long("remote")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("local")
                .long("local")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("trace")
                .long("trace")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .get_matches_from(args);

    let port = args.value_of("port").unwrap_or("8000").to_owned();
    info!("Now listening on localhost:{}", port);

    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    run_http_server(
        addr,
        port,
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
    );

    return 0;
}

fn run_http_server(addr: net::SocketAddr, port: String, local: &Path, remote: &str) {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let h2 = core.handle();
    let forward_maps = Arc::new(RwLock::new(view_maps::ViewMaps::new()));
    let backward_maps = Arc::new(RwLock::new(view_maps::ViewMaps::new()));
    let credential_cache = Arc::new(RwLock::new(CredentialCache::new()));
    let known_views = Arc::new(RwLock::new(KnownViews::new()));
    let fetching = Arc::new(RwLock::new(HashSet::new()));
    let server_handle = core.handle();
    let fetch_push_pool = CpuPool::new(8);
    let housekeeping_pool = CpuPool::new(1);
    let compute_pool = CpuPool::new(4);
    let port = port.clone();
    let remote = remote.to_owned();
    let local = local.to_owned();
    let serve = Http::new()
        .serve_addr_handle(&addr, &server_handle, move || {
            let cghttp = HttpService {
                handle: h2.clone(),
                fetch_push_pool: fetch_push_pool.clone(),
                housekeeping_pool: housekeeping_pool.clone(),
                compute_pool: compute_pool.clone(),
                port: port.clone(),
                base_path: local.clone(),
                base_url: remote.clone(),
                forward_maps: forward_maps.clone(),
                backward_maps: backward_maps.clone(),
                credential_cache: credential_cache.clone(),
                known_views: known_views.clone(),
                fetching: fetching.clone(),
            };
            Ok(cghttp)
        })
        .unwrap();

    let h2 = server_handle.clone();
    server_handle.spawn(
        serve
            .for_each(move |conn| {
                h2.spawn(
                    conn.map(|_| ())
                        .map_err(|err| warn!("serve error:: {:?}", err)),
                );
                Ok(())
            })
            .map_err(|_| ()),
    );
    core.run(futures::future::empty::<(), ()>()).unwrap();
}

fn to_ns(path: &str) -> String {
    return path.trim_matches('/').replace("/", "/refs/namespaces/");
}

fn discover_views(_headref: &str, br_path: &Path, known_views: Arc<RwLock<KnownViews>>) {
    let _trace_s = span!(Level::TRACE, "discover_views", ?br_path);

    let repo = scratch::new(&br_path);

    if let Ok(mut kn) = known_views.try_write() {
        for (prefix, kv) in kn.iter_mut() {
            let refname = format!("refs/namespaces/{}/{}", &to_ns(prefix), "refs/heads/master");
            let hs = scratch::find_all_views(&repo, &refname);
            for i in hs {
                kv.insert(i);
            }
        }
    }
}

fn make_view_repo(
    view_string: &str,
    prefix: &str,
    headref: &str,
    namespace: &str,
    br_path: &Path,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> PathBuf {
    let _trace_s = span!(Level::TRACE, "make_view_repo", ?view_string, ?br_path);

    let scratch = scratch::new(&br_path);

    let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone());
    let mut fm = view_maps::ViewMaps::new_downstream(forward_maps.clone());

    let viewobj = josh::build_view(&scratch, &view_string);

    let mut refs = vec![];

    let to_head = format!("refs/namespaces/{}/HEAD", &namespace);

    if headref != "" {
        let refname = format!("refs/namespaces/{}/{}", &to_ns(prefix), headref);
        refs.push((refname.to_owned(), to_head.clone()));
    } else {
        let glob = format!("refs/namespaces/{}/*", &to_ns(prefix));
        for refname in scratch.references_glob(&glob).unwrap().names() {
            let refname = refname.unwrap();
            let to_ref = refname.replacen(&to_ns(prefix), &namespace, 1);

            if to_ref.contains("/refs/cache-automerge/") {
                continue;
            }
            if to_ref.contains("/refs/changes/") {
                continue;
            }
            if to_ref.contains("/refs/notes/") {
                continue;
            }

            refs.push((refname.to_owned(), to_ref.clone()));
        }
    }

    scratch::apply_view_to_refs(&scratch, &*viewobj, &refs, &mut fm, &mut bm);

    if headref == "" {
        let mastername = format!("refs/namespaces/{}/refs/heads/master", &namespace);
        scratch.reference_symbolic(&to_head, &mastername, true, "");
    }

    span!(Level::TRACE, "write_lock", view_string, namespace, ?br_path).in_scope(|| {
        let mut forward_maps = forward_maps.write().unwrap();
        let mut backward_maps = backward_maps.write().unwrap();
        forward_maps.merge(&fm);
        backward_maps.merge(&bm);
    });

    br_path.to_owned()
}

fn get_info(
    view_string: &str,
    prefix: &str,
    rev: &str,
    br_path: &Path,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> String {
    let _trace_s = span!(Level::TRACE, "get_info", ?view_string, ?br_path);

    let scratch = scratch::new(&br_path);

    let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone());
    let mut fm = view_maps::ViewMaps::new_downstream(forward_maps.clone());

    let viewobj = josh::build_view(&scratch, &view_string);

    let fr = &format!("refs/namespaces/{}/{}", &to_ns(&prefix), &rev);

    let obj = ok_or!(scratch.revparse_single(&fr), {
        ok_or!(scratch.revparse_single(&rev), {
            return format!("rev not found: {:?}", &rev);
        })
    });

    let commit = ok_or!(obj.peel_to_commit(), {
        return format!("not a commit");
    });

    let mut meta = HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let transformed = viewobj.apply_view_to_commit(&scratch, &commit, &mut fm, &mut bm, &mut meta);

    let parent_ids = |commit: &git2::Commit| {
        let pids: Vec<_> = commit
            .parent_ids()
            .map(|x| {
                json!({
                    "commit": x.to_string(),
                    "tree": scratch.find_commit(x)
                        .map(|c| { c.tree_id() })
                        .unwrap_or(git2::Oid::zero())
                        .to_string(),
                })
            })
            .collect();
        pids
    };

    let t = if let Ok(transformed) = scratch.find_commit(transformed) {
        json!({
            "commit": transformed.id().to_string(),
            "tree": transformed.tree_id().to_string(),
            "parents": parent_ids(&transformed),
        })
    } else {
        json!({
            "commit": git2::Oid::zero().to_string(),
            "tree": git2::Oid::zero().to_string(),
            "parents": json!([]),
        })
    };

    let s = json!({
        "original": {
            "commit": commit.id().to_string(),
            "tree": commit.tree_id().to_string(),
            "parents": parent_ids(&commit),
        },
        "transformed": t,
    });

    return serde_json::to_string(&s).unwrap_or("Json Error".to_string());
}

fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    if args[0].ends_with("/update") {
        println!("josh-proxy");
        exit(virtual_repo::update_hook(&args[1], &args[2], &args[3]));
    }
    exit(run_proxy(args));
}
