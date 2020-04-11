#[macro_use]
extern crate josh;

extern crate clap;
extern crate data_encoding;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate tempfile;
/* extern crate hyper_tls; */
extern crate rand;
extern crate regex;

#[macro_use]
extern crate lazy_static;

extern crate bincode;
extern crate serde_json;

extern crate crypto;
extern crate serde;
extern crate tokio_core;
extern crate tracing;
extern crate tracing_log;
extern crate tracing_subscriber;

/* extern crate opentelemetry; */
/* extern crate tracing_opentelemetry; */

/* use opentelemetry::{api::Provider, sdk}; */
/* use tracing_opentelemetry::OpentelemetryLayer; */
/* use tracing_subscriber::{Layer, Registry}; */
use tracing_subscriber::Layer;

use futures::future::Future;
use futures::Stream;
use futures_cpupool::CpuPool;
use hyper::header::{Authorization, Basic};
use hyper::server::{Http, Request, Response, Service};
use josh::base_repo;
use josh::cgi;
use josh::shell;
use josh::view_maps;
use josh::virtual_repo;
use rand::random;
use regex::Regex;
use std::env;
use std::process::exit;

use crypto::digest::Digest;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::remove_dir_all;
use std::net;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use tracing::{debug, info, span, trace, warn, Level};

use tracing::*;

lazy_static! {
    static ref VIEW_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<headref>@[^:!]*)?(?P<view>[:!].*)[.](?P<ending>(?:git)|(?:json))(?P<pathinfo>/.*)?")
            .expect("can't compile regex");
    static ref CHANGE_REGEX: Regex =
        Regex::new(r"/c/(?P<change>.*)/")
            .expect("can't compile regex");
}

type CredentialCache = HashMap<String, std::time::Instant>;

/* type HttpClient = */
/*     hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>; */

type HttpClient = hyper::Client<hyper::client::HttpConnector>;

#[derive(Clone)]
struct HttpService {
    handle: tokio_core::reactor::Handle,
    fetch_push_pool: CpuPool,
    housekeeping_pool: CpuPool,
    compute_pool: CpuPool,
    port: String,
    base_path: PathBuf,
    http_client: HttpClient,
    base_url: String,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    known_views: Arc<RwLock<base_repo::KnownViews>>,
    fetching: Arc<RwLock<HashSet<String>>>,
}

fn hash_strings(url: &str, username: &str, password: &str) -> String {
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

fn to_known_view(prefix: &str, viewstr: &str) -> String {
    return format!(
        "known_views/refs/namespaces/{}/refs/namespaces/{}",
        data_encoding::BASE64URL_NOPAD.encode(prefix.as_bytes()),
        data_encoding::BASE64URL_NOPAD.encode(viewstr.as_bytes())
    );
}

fn fetch_upstream(
    http: &HttpService,
    prefix: &str,
    username: &str,
    password: &str,
    remote_url: String,
    headref: &str,
) -> Box<
    futures_cpupool::CpuFuture<
        std::result::Result<(), git2::Error>,
        hyper::Error,
    >,
> {
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
            std::time::Instant::now().duration_since(c)
                < std::time::Duration::from_secs(60)
        } else {
            false
        }
    };

    let do_fetch = if credentials_cached_ok
        && !fetching
            .write()
            .map(|mut x| x.insert(credentials_hashed.clone()))
            .unwrap_or(true)
    {
        Box::new(
            http.compute_pool
                .spawn(futures::future::ok(()).map(move |_| Ok(()))),
        )
    } else {
        Box::new(http.fetch_push_pool.spawn(futures::future::ok(()).map(
            move |_| {
                let refs_to_fetch = if headref != "" {
                    vec![headref.as_str()]
                } else {
                    vec!["refs/heads/*", "refs/tags/*"]
                };
                let credentials_hashed =
                    hash_strings(&remote_url, &username, &password);
                debug!(
                    "credentials_hashed {:?}, {:?}, {:?}",
                    &remote_url, &username, &credentials_hashed
                );
                base_repo::fetch_refs_from_url(
                    &br_path,
                    &prefix,
                    &remote_url,
                    &refs_to_fetch,
                    &username,
                    &password,
                )
                .and_then(|_| {
                    if let Ok(mut x) = fetching.write() {
                        x.remove(&credentials_hashed);
                    } else {
                        error!("lock poisoned");
                    }
                    if let Ok(mut cc) = credential_cache.write() {
                        cc.insert(
                            credentials_hashed,
                            std::time::Instant::now(),
                        );
                    } else {
                        error!("lock poisoned");
                    }
                    Ok(())
                })
            },
        )))
    };

    if credentials_cached_ok {
        do_fetch.forget();
        return Box::new(http.compute_pool.spawn(futures::future::ok(Ok(()))));
    }

    return do_fetch;
}

fn async_fetch(
    http: &HttpService,
    prefix: String,
    headref: String,
    viewstr: String,
    username: String,
    password: String,
    namespace: String,
    remote_url: String,
) -> Box<dyn Future<Item = Result<PathBuf, git2::Error>, Error = hyper::Error>>
{
    let br_path = http.base_path.clone();
    base_repo::create_local(&br_path);

    let fetch_future = fetch_upstream(
        http, &prefix, &username, &password, remote_url, &headref,
    );

    let forward_maps = http.forward_maps.clone();
    let backward_maps = http.backward_maps.clone();
    let br_path = http.base_path.clone();

    Box::new(http.compute_pool.spawn(fetch_future.map(move |r| {
        r.map(move |_| {
            let mut bm =
                view_maps::ViewMaps::new_downstream(backward_maps.clone());
            let mut fm =
                view_maps::ViewMaps::new_downstream(forward_maps.clone());
            base_repo::make_view_repo(
                &viewstr, &prefix, &headref, &namespace, &br_path, &mut fm,
                &mut bm,
            );
            span!(Level::TRACE, "write_lock backward_maps").in_scope(|| {
                backward_maps.write().map(|mut m| m.merge(&bm)).ok();
            });
            span!(Level::TRACE, "write_lock forward_maps").in_scope(|| {
                forward_maps.write().map(|mut m| m.merge(&fm)).ok();
            });
            br_path
        })
    })))
}

fn respond_unauthorized() -> Response {
    let mut response: Response =
        Response::new().with_status(hyper::StatusCode::Unauthorized);
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

fn git_command(
    cmd: String,
    br_path: PathBuf,
    pool: CpuPool,
) -> Box<dyn Future<Item = String, Error = hyper::Error>> {
    return Box::new(pool.spawn_fn(move || {
        let shell = shell::Shell {
            cwd: br_path.to_owned(),
        };
        let (stdout, _stderr) = shell.command(&cmd);
        /* println!("git_command stdout: {}", stdout); */
        /* println!("git_command stderr: {}", _stderr); */
        return futures::future::ok(stdout);
    }));
}

fn body2string(body: hyper::Chunk) -> String {
    let mut buffer: Vec<u8> = Vec::new();
    for i in body {
        buffer.push(i);
    }

    String::from_utf8(buffer).unwrap_or("".to_string())
}

fn gerrit_api(
    client: HttpClient,
    base_url: &str,
    endpoint: &str,
    query: String,
) -> Box<dyn Future<Item = serde_json::Value, Error = hyper::Error>> {
    let uri =
        hyper::Uri::from_str(&format!("{}/{}?{}", base_url, endpoint, query))
            .unwrap();

    println!("gerrit_api: {:?}", &uri);

    let auth = Authorization(Basic {
        username: env::var("JOSH_USERNAME").unwrap_or("".to_owned()),
        password: env::var("JOSH_PASSWORD").ok(),
    });

    let mut r = hyper::Request::new(hyper::Method::Get, uri);
    r.headers_mut().set(auth);
    return Box::new(
        client
            .request(r)
            .and_then(move |x| x.body().concat2().map(body2string))
            .and_then(move |resp_text| {
                println!("gerrit_api resp: {}", &resp_text);
                let v: serde_json::Value =
                    serde_json::from_str(&resp_text[4..]).unwrap();
                futures::future::ok(v)
            }),
    );
}

fn as_str(x: regex::Match) -> String {
    x.as_str().to_owned()
}

fn j2str(val: &serde_json::Value, s: &str) -> String {
    if let Some(r) = val.pointer(s) {
        return r.to_string().trim_matches('"').to_string();
    }
    return format!("## not found: {:?}", s);
}

fn call_service(
    service: &HttpService,
    req: Request,
    namespace: &str,
) -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
    let s1 = span!(Level::TRACE, "j call_service");
    let _e1 = s1.enter();
    let s2 = span!(Level::TRACE, "j2 call_service");
    let _e2 = s2.enter();
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    let br_path = service.base_path.clone();

    if path == "/version" {
        let response = Response::new()
            .with_body(format!(
                "Version: {}\n",
                option_env!("GIT_DESCRIBE")
                    .unwrap_or(env!("CARGO_PKG_VERSION"))
            ))
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
    if path == "/flush" {
        service.credential_cache.write().unwrap().clear();
        let response = Response::new()
            .with_body(format!("Flushed credential cache\n"))
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if path == "/views" {
        service.credential_cache.write().unwrap().clear();

        let known_views = service.known_views.clone();
        let discover = service
            .compute_pool
            .spawn_fn(move || {
                base_repo::discover_views(
                    &br_path.clone(),
                    known_views.clone(),
                );
                Ok(known_views)
            })
            .map(move |known_views| {
                let body =
                    toml::to_string_pretty(&*known_views.read().unwrap())
                        .unwrap();
                Response::new()
                    .with_body(body)
                    .with_status(hyper::StatusCode::Ok)
            });
        return Box::new(discover);
    }
    if path == "/panic" {
        panic!();
    }
    if path == "/repo_update" {
        let pool = service.fetch_push_pool.clone();
        return Box::new(
            req.body()
                .concat2()
                .map(body2string)
                .and_then(move |buffer| {
                    return pool.spawn(futures::future::ok(buffer).map(
                        move |buffer| {
                            let repo_update: virtual_repo::RepoUpdate =
                                serde_json::from_str(&buffer)
                                    .unwrap_or(virtual_repo::RepoUpdate::new());
                            virtual_repo::process_repo_update(
                                repo_update,
                                forward_maps,
                                backward_maps,
                            )
                        },
                    ));
                })
                .and_then(move |result| {
                    if let Ok(stderr) = result {
                        let response = Response::new()
                            .with_body(stderr)
                            .with_status(hyper::StatusCode::Ok);
                        return Box::new(futures::future::ok(response));
                    }
                    if let Err(josh::JoshError(stderr)) = result {
                        let response = Response::new()
                            .with_body(stderr)
                            .with_status(hyper::StatusCode::BadRequest);
                        return Box::new(futures::future::ok(response));
                    }
                    let response = Response::new()
                        .with_status(hyper::StatusCode::Forbidden);
                    return Box::new(futures::future::ok(response));
                }),
        );
    }

    if path.starts_with("/static/") {
        return Box::new(
            service.http_client.get(
                hyper::Uri::from_str(&format!(
                    "http://localhost:3000{}",
                    &path
                ))
                .unwrap(),
            ),
        );
    }

    if let Some(caps) = CHANGE_REGEX.captures(&path) {
        let change = caps.name("change").map(as_str).unwrap_or("".to_owned());
        let pool = service.housekeeping_pool.clone();
        let br_path = br_path.clone();
        let client = service.http_client.clone();

        let get_comments = gerrit_api(
            client.clone(),
            &service.base_url,
            &format!("/a/changes/{}/comments", change),
            format!(""),
        );

        let r = gerrit_api(
            client.clone(),
            &service.base_url,
            "/a/changes/",
            format!("q=change:{}&o=ALL_REVISIONS&o=ALL_COMMITS", change),
        )
        .and_then(move |change_json| {
            let to = j2str(&change_json, "/0/current_revision");
            let from = j2str(
                &change_json,
                &format!("/0/revisions/{}/commit/parents/0/commit", &to),
            );
            let mut resp = HashMap::<String, String>::new();
            let cmd = format!("git diff -U99999999 {}..{}", from, to);
            println!("diffcmd: {:?}", cmd);
            git_command(cmd, br_path.to_owned(), pool.clone()).and_then(
                move |stdout| {
                    resp.insert("diff".to_owned(), stdout);
                    futures::future::ok((resp, change_json))
                },
            )
        })
        .and_then(move |(resp, change_json)| {
            let mut revision2sha = HashMap::<i64, String>::new();
            for (k, v) in
                change_json[0]["revisions"].as_object().unwrap().iter()
            {
                revision2sha
                    .insert(v["_number"].as_i64().unwrap(), k.to_string());
            }

            get_comments.and_then(move |comments_value| {
                for i in comments_value.as_object().unwrap().keys() {
                    println!("comments_value: {:?}", &i);
                }

                let response = Response::new()
                    .with_body(serde_json::to_string(&resp).unwrap())
                    .with_status(hyper::StatusCode::Ok);
                futures::future::ok(response)
            })
        });

        return Box::new(r);
    };

    let (prefix, view_string, pathinfo, headref, ending) =
        some_or!(parse_url(&path), {
            return Box::new(
                service.http_client.get(
                    hyper::Uri::from_str("http://localhost:3000").unwrap(),
                ),
            );
        });

    let headref = headref.trim_start_matches("@").to_owned();

    let compute_pool = service.compute_pool.clone();

    if ending == "json" {
        let forward_maps = service.forward_maps.clone();
        let backward_maps = service.forward_maps.clone();
        let br_path = service.base_path.clone();

        let f = compute_pool.spawn(futures::future::ok(true).map(move |_| {
            let info = base_repo::get_info(
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

    let call_git_http_backend =
        |request: Request,
         path: PathBuf,
         pathinfo: &str,
         handle: &tokio_core::reactor::Handle|
         -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
            trace!("git-http backend {:?} {:?}", path, pathinfo);
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

    let handle = service.handle.clone();
    let ns_path = service.base_path.clone();
    let ns_path = ns_path.join("refs/namespaces");
    let ns_path = ns_path.join(&namespace);
    assert!(namespace.contains("request_"));

    let known_views = service.known_views.clone();
    let prefix2 = prefix.clone();
    let viewstr2 = view_string.clone();

    service
        .compute_pool
        .spawn_fn(move || -> Result<(), ()> {
            if let Ok(mut kn) = known_views.write() {
                kn.entry(prefix2.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(viewstr2);
            } else {
                warn!("Can't lock 'known_views' for writing");
            }
            Ok(())
        })
        .forget();

    Box::new({
        async_fetch(
            &service,
            prefix.to_string(),
            headref.to_string(),
            view_string.to_string(),
            username.to_string(),
            password.to_string(),
            namespace.to_string(),
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
            remove_dir_all(ns_path).unwrap_or_else(|e| warn!("remove_dir_all failed: {:?}", e));
            x
        })
    })
}

fn without_password(headers: &hyper::Headers) -> hyper::Headers {
    let username = match headers.get() {
        Some(&Authorization(Basic {
            ref username,
            password: _,
        })) => username.to_owned(),
        None => "".to_owned(),
    };
    let mut headers = headers.clone();
    headers.set(Authorization(Basic {
        username: username,
        password: None,
    }));
    return headers;
}

impl Service for HttpService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let rid: usize = random();
        let rname = format!("request_{}", rid);

        let _trace_s = span!(
            Level::TRACE, "call_service", path = ?req.path(), headers = ?without_password(req.headers()));
        Box::new(call_service(&self, req, &rname).map(move |response| {
            event!(parent: &_trace_s, Level::TRACE, ?response);
            response
        }))
    }
}

fn parse_args(args: &[String]) -> clap::ArgMatches {
    clap::App::new("josh-proxy")
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
        .arg(clap::Arg::with_name("gc").long("gc").takes_value(false))
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .get_matches_from(args)
}

fn run_proxy(args: Vec<String>) -> josh::JoshResult<i32> {
    tracing_log::LogTracer::init()?;

    /* let tracer = sdk::Provider::default().get_tracer("josh-proxy"); */
    /* let layer = OpentelemetryLayer::with_tracer(tracer); */

    /* let subscriber = layer.with_subscriber(Registry::default()); */

    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    let filter = tracing_subscriber::filter::EnvFilter::new(
        "josh_proxy=trace,josh=trace",
    );
    let subscriber = filter.with_subscriber(subscriber);
    tracing::subscriber::set_global_default(subscriber)?;

    debug!("RUN PROXY {:?}", &args);

    let args = parse_args(&args);

    let port = args.value_of("port").unwrap_or("8000").to_owned();
    println!("Now listening on localhost:{}", port);

    let mut core = tokio_core::reactor::Core::new()?;

    let addr = format!("0.0.0.0:{}", port).parse()?;
    let service = run_http_server(
        &mut core,
        addr,
        port,
        &PathBuf::from(
            args.value_of("local").expect("missing local directory"),
        ),
        &args.value_of("remote").expect("missing remote repo url"),
    )?;

    let known_views = service.known_views.clone();
    let br_path = service.base_path.clone();
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();
    let do_gc = args.is_present("gc");
    let mut gc_timer = std::time::Instant::now();
    let mut persist_timer =
        std::time::Instant::now() - std::time::Duration::from_secs(60 * 5);

    std::thread::spawn(move || {
        let mut total = 0;
        loop {
            base_repo::discover_views(&br_path.clone(), known_views.clone());
            if let Ok(kn) = known_views.read() {
                for (prefix2, e) in kn.iter() {
                    info!("background rebuild root: {:?}", prefix2);

                    let mut bm = view_maps::ViewMaps::new_downstream(
                        backward_maps.clone(),
                    );
                    let mut fm = view_maps::ViewMaps::new_downstream(
                        forward_maps.clone(),
                    );

                    let mut updated_count = 0;

                    for v in e.iter() {
                        trace!("background rebuild: {:?} {:?}", prefix2, v);

                        updated_count += base_repo::make_view_repo(
                            &v,
                            &prefix2,
                            "refs/heads/master",
                            &to_known_view(&prefix2, &v),
                            &br_path,
                            &mut fm,
                            &mut bm,
                        );
                    }
                    info!("updated {} refs for {:?}", updated_count, prefix2);

                    let stats = fm.stats();
                    total += fm.stats()["total"];
                    total += bm.stats()["total"];
                    info!(
                        "forward_maps stats: {}",
                        toml::to_string_pretty(&stats).unwrap()
                    );
                    span!(Level::TRACE, "write_lock bm").in_scope(|| {
                        let mut backward_maps = backward_maps.write().unwrap();
                        backward_maps.merge(&bm);
                    });
                    span!(Level::TRACE, "write_lock fm").in_scope(|| {
                        let mut forward_maps = forward_maps.write().unwrap();
                        forward_maps.merge(&fm);
                    });
                }
            }
            if total > 1000
                || persist_timer.elapsed()
                    > std::time::Duration::from_secs(60 * 15)
            {
                view_maps::persist(
                    &*backward_maps.read().unwrap(),
                    &br_path.join("josh_backward_maps"),
                );
                view_maps::persist(
                    &*forward_maps.read().unwrap(),
                    &br_path.join("josh_forward_maps"),
                );
                total = 0;
                persist_timer = std::time::Instant::now();
            }
            info!(
                "{}",
                base_repo::run_housekeeping(&br_path, &"git count-objects -v")
                    .replace("\n", "  ")
            );
            if do_gc
                && gc_timer.elapsed() > std::time::Duration::from_secs(60 * 60)
            {
                info!(
                    "\n----------\n{}\n----------",
                    base_repo::run_housekeeping(&br_path, &"git repack -adkbn")
                );
                info!(
                    "\n----------\n{}\n----------",
                    base_repo::run_housekeeping(
                        &br_path,
                        &"git count-objects -vH"
                    )
                );
                info!(
                    "\n----------\n{}\n----------",
                    base_repo::run_housekeeping(
                        &br_path,
                        &"git prune --expire=2w"
                    )
                );
                gc_timer = std::time::Instant::now();
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    });

    core.run(futures::future::empty::<(), josh::JoshError>())?;

    Ok(0)
}

fn run_http_server(
    core: &mut tokio_core::reactor::Core,
    addr: net::SocketAddr,
    port: String,
    local: &Path,
    remote: &str,
) -> josh::JoshResult<HttpService> {
    let cghttp = HttpService {
        handle: core.handle(),
        fetch_push_pool: CpuPool::new(8),
        housekeeping_pool: CpuPool::new(1),
        compute_pool: CpuPool::new(4),
        port: port,
        base_path: local.to_owned(),
        /* http_client: hyper::Client::configure() */
        /*     .connector( */
        /*         ::hyper_tls::HttpsConnector::new(4, &core.handle()).unwrap(), */
        /*     ) */
        /*     .keep_alive(true) */
        /*     .build(&core.handle()), */
        http_client: hyper::Client::new(&core.handle()),
        forward_maps: Arc::new(RwLock::new(view_maps::try_load(
            &local.join("josh_forward_maps"),
        ))),
        backward_maps: Arc::new(RwLock::new(view_maps::try_load(
            &local.join("josh_backward_maps"),
        ))),
        base_url: remote.to_owned(),
        credential_cache: Arc::new(RwLock::new(CredentialCache::new())),
        known_views: Arc::new(RwLock::new(base_repo::KnownViews::new())),
        fetching: Arc::new(RwLock::new(HashSet::new())),
    };

    let service = cghttp.clone();

    let server_handle = core.handle();
    let serve =
        Http::new().serve_addr_handle(&addr, &server_handle, move || {
            Ok(cghttp.clone())
        })?;

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

    return Ok(service);
}

fn to_ns(path: &str) -> String {
    return path.trim_matches('/').replace("/", "/refs/namespaces/");
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
        exit(
            virtual_repo::update_hook(&args[1], &args[2], &args[3])
                .unwrap_or(1),
        );
    }
    exit(run_proxy(args).unwrap_or(1));
}
