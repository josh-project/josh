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
use josh::housekeeping;
use josh::shell;
use josh::view_maps;
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
        Regex::new(r"(?P<upstream_repo>/.*[.]git)(?P<headref>@[^:!]*)?(?P<view>[:!].*)[.](?P<ending>(?:git)|(?:json))(?P<pathinfo>/.*)?")
            .expect("can't compile regex");
    static ref CHANGE_REGEX: Regex =
        Regex::new(r"/c/(?P<change>.*)/")
            .expect("can't compile regex");
}

type CredentialCache = HashMap<String, std::time::Instant>;

type BoxedFuture<T> = Box<dyn Future<Item = T, Error = hyper::Error>>;

/* type HttpClient = */
/*     hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>; */

type HttpClient = hyper::Client<hyper::client::HttpConnector>;

#[derive(Clone)]
struct JoshProxyService {
    handle: tokio_core::reactor::Handle,
    fetch_push_pool: CpuPool,
    housekeeping_pool: CpuPool,
    compute_pool: CpuPool,
    port: String,
    repo_path: PathBuf,
    http_client: HttpClient,
    upstream_url: String,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    known_views: Arc<RwLock<housekeeping::KnownViews>>,
    fetching: Arc<RwLock<HashSet<String>>>,
}

fn hash_strings(url: &str, username: &str, password: &str) -> String {
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

fn fetch_upstream_ref(
    service: JoshProxyService,
    upstream_repo: String,
    username: String,
    password: String,
    remote_url: String,
    headref: String,
) -> Box<futures_cpupool::CpuFuture<bool, hyper::Error>> {
    let repo_path = service.repo_path.clone();

    Box::new(service.fetch_push_pool.spawn_fn(move || {
        futures::future::ok(
            josh_proxy::fetch_refs_from_url(
                &repo_path,
                &upstream_repo,
                &remote_url,
                &[headref.as_str()],
                &username,
                &password,
            )
            .is_ok(),
        )
    }))
}

fn fetch_upstream(
    service: JoshProxyService,
    upstream_repo: String,
    username: String,
    password: String,
    remote_url: String,
) -> Box<futures_cpupool::CpuFuture<bool, hyper::Error>> {
    let credentials_hashed = hash_strings(&remote_url, &username, &password);

    debug!(
        "credentials_hashed {:?}, {:?}, {:?}",
        &remote_url, &username, &credentials_hashed
    );

    let credentials_cached_ok = {
        let last = service
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
    let refs_to_fetch = vec!["refs/heads/*", "refs/tags/*"];

    let do_fetch = if credentials_cached_ok
        && !service
            .fetching
            .write()
            .map(|mut x| x.insert(credentials_hashed.clone()))
            .unwrap_or(true)
    {
        Box::new(service.compute_pool.spawn(futures::future::ok(true)))
    } else {
        let credential_cache = service.credential_cache.clone();
        let br_path = service.repo_path.clone();
        let fetching = service.fetching.clone();
        Box::new(service.fetch_push_pool.spawn_fn(move || {
            if let Ok(_) = josh_proxy::fetch_refs_from_url(
                &br_path,
                &upstream_repo,
                &remote_url,
                &refs_to_fetch,
                &username,
                &password,
            ) {
                if let Ok(mut x) = fetching.write() {
                    x.remove(&credentials_hashed);
                } else {
                    error!("lock poisoned");
                }
                if let Ok(mut cc) = credential_cache.write() {
                    cc.insert(credentials_hashed, std::time::Instant::now());
                } else {
                    error!("lock poisoned");
                }
                futures::future::ok(true)
            } else {
                futures::future::ok(false)
            }
        }))
    };

    if credentials_cached_ok {
        do_fetch.forget();
        return Box::new(service.compute_pool.spawn(futures::future::ok(true)));
    }

    return do_fetch;
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
        caps.name("upstream_repo")
            .map(as_str)
            .unwrap_or("".to_owned()),
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
) -> BoxedFuture<String> {
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
    upstream_url: &str,
    endpoint: &str,
    query: String,
) -> BoxedFuture<serde_json::Value> {
    let uri = hyper::Uri::from_str(&format!(
        "{}/{}?{}",
        upstream_url, endpoint, query
    ))
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
    service: &JoshProxyService,
    req: Request,
    namespace: &str,
) -> BoxedFuture<Response> {
    let s1 = span!(Level::TRACE, "j call_service");
    let _e1 = s1.enter();
    let s2 = span!(Level::TRACE, "j2 call_service");
    let _e2 = s2.enter();
    let repo = git2::Repository::init_bare(&service.repo_path).unwrap();

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

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
                housekeeping::discover_filter_candidates(
                    &repo,
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
        let forward_maps = service.forward_maps.clone();
        let backward_maps = service.backward_maps.clone();
        let response = req
            .body()
            .concat2()
            .map(body2string)
            .and_then(move |buffer| {
                pool.spawn(futures::future::ok(buffer).map(move |buffer| {
                    josh_proxy::process_repo_update(
                        serde_json::from_str(&buffer).unwrap_or(HashMap::new()),
                        forward_maps,
                        backward_maps,
                    )
                }))
            })
            .and_then(move |result| {
                let response = if let Ok(stderr) = result {
                    Response::new()
                        .with_body(stderr)
                        .with_status(hyper::StatusCode::Ok)
                } else if let Err(josh::JoshError(stderr)) = result {
                    Response::new()
                        .with_body(stderr)
                        .with_status(hyper::StatusCode::BadRequest)
                } else {
                    Response::new().with_status(hyper::StatusCode::Forbidden)
                };
                futures::future::ok(response)
            });
        return Box::new(response);
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
        let client = service.http_client.clone();

        let get_comments = gerrit_api(
            client.clone(),
            &service.upstream_url,
            &format!("/a/changes/{}/comments", change),
            format!(""),
        );

        let br_path = service.repo_path.clone();
        let r = gerrit_api(
            client.clone(),
            &service.upstream_url,
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

    let (upstream_repo, view_string, pathinfo, headref, ending) =
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

        let f = compute_pool.spawn(futures::future::ok(true).map(move |_| {
            housekeeping::get_info(
                &repo,
                &josh::filters::parse(&view_string),
                &upstream_repo,
                &headref,
                forward_maps.clone(),
                backward_maps.clone(),
            )
            .unwrap_or("get_info: error".to_owned())
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
    let filter_spec = view_string.clone();
    let ns = namespace.to_owned();

    let port = service.port.clone();

    let remote_url = {
        let mut remote_url = service.upstream_url.clone();
        remote_url.push_str(&upstream_repo);
        remote_url
    };

    let br_url = remote_url.clone();
    let base_ns = josh::to_ns(&upstream_repo);

    let call_git_http_backend = |request: Request,
                                 path: PathBuf,
                                 pathinfo: &str,
                                 handle: &tokio_core::reactor::Handle|
     -> BoxedFuture<Response> {
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
        cmd.env("JOSH_VIEWSTR", filter_spec);
        cmd.env("JOSH_REMOTE", remote_url);
        cmd.env("JOSH_BASE_NS", base_ns);

        josh_proxy::do_cgi(request, cmd, handle.clone())
    };

    let handle = service.handle.clone();

    let request_tmp_namespace =
        service.repo_path.join("refs/namespaces").join(&namespace);

    remember_known_filter_spec(
        &service,
        upstream_repo.clone(),
        view_string.clone(),
    );

    let fetch_future = if headref == "" {
        fetch_upstream(
            service.clone(),
            upstream_repo.clone(),
            username,
            password,
            br_url,
        )
    } else {
        fetch_upstream_ref(
            service.clone(),
            upstream_repo.clone(),
            username,
            password,
            br_url,
            headref.clone(),
        )
    };

    let namespace = namespace.to_owned();
    let filter_spec = view_string.clone();
    let service = service.clone();

    let fetch_future =
        fetch_future.and_then(move |authorized| -> BoxedFuture<Response> {
            if !authorized {
                debug!("wrong credentials");
                return Box::new(futures::future::ok(respond_unauthorized()));
            }

            let do_filter = do_filter(
                repo,
                &service,
                upstream_repo,
                namespace,
                filter_spec,
                headref,
            );

            let response = do_filter.and_then(move |_| {
                call_git_http_backend(
                    req,
                    service.repo_path,
                    &pathinfo,
                    &handle,
                )
            });

            Box::new(response)
        });

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    Box::new({
        fetch_future.map(move |response| {
            remove_dir_all(request_tmp_namespace)
                .unwrap_or_else(|e| warn!("remove_dir_all failed: {:?}", e));
            response
        })
    })
}

fn do_filter(
    repo: git2::Repository,
    service: &JoshProxyService,
    upstream_repo: String,
    namespace: String,
    filter_spec: String,
    headref: String,
) -> BoxedFuture<()> {
    let pool = service.compute_pool.clone();
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();
    let r = pool.spawn_fn(move || {
        let mut bm = view_maps::new_downstream(&backward_maps);
        let mut fm = view_maps::new_downstream(&forward_maps);
        housekeeping::make_view_repo(
            &repo,
            &*josh::filters::parse(&filter_spec),
            &upstream_repo,
            &headref,
            &namespace,
            &mut fm,
            &mut bm,
        );
        view_maps::try_merge_both(forward_maps, backward_maps, &fm, &bm);
        return futures::future::ok(());
    });
    return Box::new(r);
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

impl Service for JoshProxyService {
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

    let repo = josh_proxy::create_repo(&service.repo_path)?;
    housekeeping::spawn_thread(
        repo,
        service.known_views.clone(),
        service.forward_maps.clone(),
        service.backward_maps.clone(),
        args.is_present("gc"),
    );

    core.run(futures::future::empty::<(), josh::JoshError>())?;

    Ok(0)
}

fn run_http_server(
    core: &mut tokio_core::reactor::Core,
    addr: net::SocketAddr,
    port: String,
    local: &Path,
    remote: &str,
) -> josh::JoshResult<JoshProxyService> {
    let service = JoshProxyService {
        handle: core.handle(),
        fetch_push_pool: CpuPool::new(8),
        housekeeping_pool: CpuPool::new(1),
        compute_pool: CpuPool::new(4),
        port: port,
        repo_path: local.to_owned(),
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
        upstream_url: remote.to_owned(),
        credential_cache: Arc::new(RwLock::new(CredentialCache::new())),
        known_views: Arc::new(RwLock::new(housekeeping::KnownViews::new())),
        fetching: Arc::new(RwLock::new(HashSet::new())),
    };

    let service2 = service.clone();

    let server_handle = core.handle();
    let serve =
        Http::new().serve_addr_handle(&addr, &server_handle, move || {
            Ok(service2.clone())
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

fn update_hook(refname: &str, old: &str, new: &str) -> josh::JoshResult<i32> {
    let mut repo_update = HashMap::new();
    repo_update.insert("new".to_owned(), new.to_owned());
    repo_update.insert("old".to_owned(), old.to_owned());
    repo_update.insert("refname".to_owned(), refname.to_owned());

    for (env_name, name) in [
        ("JOSH_USERNAME", "username"),
        ("JOSH_PASSWORD", "password"),
        ("JOSH_REMOTE", "remote_url"),
        ("JOSH_BASE_NS", "base_ns"),
        ("JOSH_VIEWSTR", "filter_spec"),
        ("GIT_NAMESPACE", "GIT_NAMESPACE"),
    ]
    .iter()
    {
        repo_update.insert(name.to_string(), env::var(&env_name)?);
    }

    repo_update.insert(
        "GIT_DIR".to_owned(),
        git2::Repository::init_bare(&Path::new(&env::var("GIT_DIR")?))?
            .path()
            .to_str()
            .ok_or(josh::josh_error("GIT_DIR not set"))?
            .to_owned(),
    );

    let port = env::var("JOSH_PORT")?;

    let client = reqwest::Client::builder().timeout(None).build()?;
    let resp = client
        .post(&format!("http://localhost:{}/repo_update", port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(mut r) => {
            if let Ok(body) = r.text() {
                println!("response from upstream:\n {}\n\n", body);
            } else {
                println!("no upstream response");
            }
            if r.status().is_success() {
                return Ok(0);
            } else {
                return Ok(1);
            }
        }
        Err(err) => {
            warn!("/repo_update request failed {:?}", err);
        }
    };
    return Ok(1);
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
        exit(update_hook(&args[1], &args[2], &args[3]).unwrap_or(1));
    }
    exit(run_proxy(args).unwrap_or(1));
}

fn remember_known_filter_spec(
    service: &JoshProxyService,
    upstream_repo: String,
    filter_spec: String,
) {
    let known_views = service.known_views.clone();
    service
        .compute_pool
        .spawn_fn(move || -> Result<(), ()> {
            if let Ok(mut kn) = known_views.write() {
                kn.entry(upstream_repo)
                    .or_insert_with(BTreeSet::new)
                    .insert(filter_spec);
            } else {
                warn!("Can't lock 'known_views' for writing");
            }
            Ok(())
        })
        .forget();
}
