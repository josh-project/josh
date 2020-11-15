#[macro_use]
extern crate lazy_static;

use futures::future::Future;
use futures::Stream;
use hyper::server::{Request, Response};
use josh_proxy::BoxedFuture;
use tracing_subscriber::Layer;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tracing::*;

fn version_str() -> String {
    format!(
        "Version: {}\n",
        option_env!("GIT_DESCRIBE").unwrap_or(std::env!("CARGO_PKG_VERSION"))
    )
}

lazy_static! {
    static ref ARGS: clap::ArgMatches<'static> = parse_args();
}

josh::regex_parsed!(
    TransformedRepoUrl,
    r"(?P<upstream_repo>/.*[.]git)(?P<headref>@[^:!]*)?(?P<view>[:!].*)[.](?P<ending>(?:git)|(?:json))(?P<pathinfo>/.*)?",
    [upstream_repo, view, pathinfo, headref, ending]
);

type CredentialCache = HashMap<String, std::time::Instant>;

#[derive(Clone)]
struct JoshProxyService {
    handle: tokio_core::reactor::Handle,
    fetch_push_pool: futures_cpupool::CpuPool,
    compute_pool: futures_cpupool::CpuPool,
    port: String,
    repo_path: std::path::PathBuf,
    /* gerrit: Arc<josh_proxy::gerrit::Gerrit>, */
    upstream_url: String,
    forward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    fetching: Arc<RwLock<std::collections::HashSet<String>>>,
}

fn parse_args() -> clap::ArgMatches<'static> {
    let args = {
        let mut args = vec![];
        for arg in std::env::args() {
            args.push(arg);
        }
        args
    };

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
        .arg(
            clap::Arg::with_name("gc")
                .long("gc")
                .takes_value(false)
                .help("Run git gc in maintanance"),
        )
        .arg(
            clap::Arg::with_name("m")
                .short("m")
                .takes_value(false)
                .help("Only run maintance and exit"),
        )
        .arg(
            clap::Arg::with_name("n").short("n").takes_value(true).help(
                "Number of concurrent upstream git fetch/push operations",
            ),
        )
        /* .arg( */
        /*     clap::Arg::with_name("g") */
        /*         .short("g") */
        /*         .takes_value(false) */
        /*         .help("Enable gerrit integration"), */
        /* ) */
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .get_matches_from(args)
}

fn hash_strings(url: &str, username: &str, password: &str) -> String {
    use crypto::digest::Digest;
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

fn fetch_upstream_ref(
    service: JoshProxyService,
    upstream_repo: String,
    username: &str,
    password: &str,
    remote_url: String,
    headref: String,
) -> Box<futures_cpupool::CpuFuture<bool, hyper::Error>> {
    let repo_path = service.repo_path.clone();

    let username = username.to_owned();
    let password = password.to_owned();

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
    username: &str,
    password: &str,
    remote_url: String,
) -> Box<futures_cpupool::CpuFuture<bool, hyper::Error>> {
    let username = username.to_owned();
    let password = password.to_owned();
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

fn static_paths(
    service: &JoshProxyService,
    path: &str,
) -> Option<BoxedFuture<Response>> {
    if path == "/version" {
        let response = Response::new()
            .with_body(version_str())
            .with_status(hyper::StatusCode::Ok);
        return Some(Box::new(futures::future::ok(response)));
    }
    if path == "/flush" {
        service.credential_cache.write().unwrap().clear();
        let response = Response::new()
            .with_body(format!("Flushed credential cache\n"))
            .with_status(hyper::StatusCode::Ok);
        return Some(Box::new(futures::future::ok(response)));
    }
    if path == "/views" {
        service.credential_cache.write().unwrap().clear();

        let repo = git2::Repository::init_bare(&service.repo_path).unwrap();
        let discover = service.compute_pool.spawn_fn(move || {
            let known_filters =
                josh::housekeeping::discover_filter_candidates(&repo).ok();
            let body = toml::to_string_pretty(&known_filters).unwrap();
            let response = Response::new()
                .with_body(body)
                .with_status(hyper::StatusCode::Ok);
            futures::future::ok(response)
        });
        return Some(Box::new(discover));
    }
    return None;
}

fn call_service(
    service: &JoshProxyService,
    req: Request,
) -> BoxedFuture<Response> {
    let repo = git2::Repository::init_bare(&service.repo_path).unwrap();

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if let Some(r) = static_paths(&service, &path) {
        return r;
    }

    if path == "/repo_update" {
        let pool = service.fetch_push_pool.clone();
        let forward_maps = service.forward_maps.clone();
        let backward_maps = service.backward_maps.clone();
        let response = req
            .body()
            .concat2()
            .map(josh_proxy::body2string)
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

    /* if path.starts_with("/review/") || path.starts_with("/c/") { */
    /*     return service.gerrit.handle_request(req); */
    /* } */

    let parsed_url = {
        let nop_path = path.replacen(".git", ".git:nop.git", 1);
        if let Some(parsed_url) = TransformedRepoUrl::from_str(&path) {
            parsed_url
        } else if let Some(parsed_url) = TransformedRepoUrl::from_str(&nop_path)
        {
            parsed_url
        } else {
            return Box::new(futures::future::ok(
                Response::new().with_status(hyper::StatusCode::NotFound),
            ));
        }
    };

    let headref = parsed_url.headref.trim_start_matches("@").to_owned();

    let compute_pool = service.compute_pool.clone();

    if parsed_url.ending == "json" {
        let forward_maps = service.forward_maps.clone();
        let backward_maps = service.forward_maps.clone();

        let f = compute_pool.spawn_fn(move || {
            let info = josh::housekeeping::get_info(
                &repo,
                &*josh::filters::parse(&parsed_url.view),
                &parsed_url.upstream_repo,
                &headref,
                forward_maps.clone(),
                backward_maps.clone(),
            )
            .unwrap_or("get_info: error".to_owned());

            let response = Response::new()
                .with_body(format!("{}\n", info))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        });

        return Box::new(f);
    }

    let (username, password) = josh::some_or!(josh_proxy::parse_auth(&req), {
        ("".to_owned(), "".to_owned())
    });

    let port = service.port.clone();

    let remote_url = [
        service.upstream_url.as_str(),
        parsed_url.upstream_repo.as_str(),
    ]
    .join("");

    let br_url = remote_url.clone();
    let base_ns = josh::to_ns(&parsed_url.upstream_repo);
    let handle = service.handle.clone();

    let fetch_future = if headref == "" {
        fetch_upstream(
            service.clone(),
            parsed_url.upstream_repo.clone(),
            &username,
            &password,
            br_url,
        )
    } else {
        fetch_upstream_ref(
            service.clone(),
            parsed_url.upstream_repo.clone(),
            &username,
            &password,
            br_url,
            headref.clone(),
        )
    };

    let temp_ns =
        Arc::new(josh_proxy::TmpGitNamespace::new(&service.repo_path));

    let refs = if headref != "" {
        Some(vec![(
            format!(
                "refs/josh/upstream/{}/{}",
                &josh::to_ns(&parsed_url.upstream_repo),
                headref
            ),
            temp_ns.reference("refs/heads/master"),
        )])
    } else {
        None
    };

    let filter_spec = parsed_url.view.clone();
    let service = service.clone();
    let fs = filter_spec.clone();

    let ns = temp_ns.clone();

    let fetch_future =
        fetch_future.and_then(move |authorized| -> BoxedFuture<Response> {
            if !authorized {
                return Box::new(futures::future::ok(
                    josh_proxy::respond_unauthorized(),
                ));
            }

            let do_filter = do_filter(
                repo,
                &service,
                parsed_url.upstream_repo,
                ns.clone(),
                filter_spec,
                refs,
            );

            let pathinfo = parsed_url.pathinfo.clone();
            let mut cmd = std::process::Command::new("git");
            let repo_path = service.repo_path.to_str().unwrap();
            cmd.arg("http-backend");
            cmd.current_dir(&service.repo_path);
            cmd.env("GIT_PROJECT_ROOT", repo_path);
            cmd.env("GIT_DIR", repo_path);
            cmd.env("GIT_HTTP_EXPORT_ALL", "");
            cmd.env("PATH_INFO", pathinfo);
            cmd.env("JOSH_PASSWORD", password);
            cmd.env("JOSH_USERNAME", username);
            cmd.env("JOSH_PORT", port);
            cmd.env("GIT_NAMESPACE", ns.name().clone());
            cmd.env("JOSH_VIEWSTR", fs);
            cmd.env("JOSH_REMOTE", remote_url);
            cmd.env("JOSH_BASE_NS", base_ns);

            let response = do_filter.and_then(move |_| {
                tracing::trace!("git-http backend {:?}", path);
                josh_proxy::do_cgi(req, cmd, handle.clone())
            });

            Box::new(response)
        });

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    Box::new({
        fetch_future.map(move |response| {
            std::mem::drop(temp_ns);
            response
        })
    })
}

fn do_filter(
    repo: git2::Repository,
    service: &JoshProxyService,
    upstream_repo: String,
    temp_ns: Arc<josh_proxy::TmpGitNamespace>,
    filter_spec: String,
    from_to: Option<Vec<(String, String)>>,
) -> BoxedFuture<git2::Repository> {
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();
    let r = service.compute_pool.spawn_fn(move || {
        let filter = josh::filters::parse(&filter_spec);
        let filter_spec = filter.filter_spec();

        let from_to = from_to.unwrap_or_else(|| {
            josh::housekeeping::default_from_to(
                &repo,
                &temp_ns.name(),
                &upstream_repo,
                &filter_spec,
            )
        });

        let mut bm = josh::view_maps::new_downstream(&backward_maps);
        let mut fm = josh::view_maps::new_downstream(&forward_maps);
        josh::scratch::apply_filter_to_refs(
            &repo, &*filter, &from_to, &mut fm, &mut bm,
        );
        josh::view_maps::try_merge_both(forward_maps, backward_maps, &fm, &bm);
        repo.reference_symbolic(
            &temp_ns.reference("HEAD"),
            &temp_ns.reference("refs/heads/master"),
            true,
            "",
        )
        .ok();
        return futures::future::ok(repo);
    });
    return Box::new(r);
}

fn without_password(headers: &hyper::Headers) -> hyper::Headers {
    let username = match headers.get() {
        Some(&hyper::header::Authorization(hyper::header::Basic {
            ref username,
            password: _,
        })) => username.to_owned(),
        None => "".to_owned(),
    };
    let mut headers = headers.clone();
    headers.set(hyper::header::Authorization(hyper::header::Basic {
        username: username,
        password: None,
    }));
    return headers;
}

impl hyper::server::Service for JoshProxyService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let _trace_s = tracing::span!(
            tracing::Level::TRACE, "call_service", path = ?req.path(), headers = ?without_password(req.headers()));
        Box::new(call_service(&self, req).map(move |response| {
            event!(parent: &_trace_s, tracing::Level::TRACE, ?response);
            response
        }))
    }
}

fn run_proxy() -> josh::JoshResult<i32> {
    tracing_log::LogTracer::init()?;
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let filter = tracing_subscriber::filter::EnvFilter::new(
        "josh_proxy=trace,josh=trace",
    );
    let subscriber = filter.with_subscriber(subscriber);
    tracing::subscriber::set_global_default(subscriber)?;

    let local = std::path::PathBuf::from(
        ARGS.value_of("local").expect("missing local directory"),
    );

    josh_proxy::create_repo(&local)?;

    let forward_maps = Arc::new(RwLock::new(josh::view_maps::try_load(
        &local.join("josh_forward_maps"),
    )));
    let backward_maps = Arc::new(RwLock::new(josh::view_maps::try_load(
        &local.join("josh_backward_maps"),
    )));

    if ARGS.is_present("m") {
        let repo = git2::Repository::init_bare(&local)?;
        let known_filters =
            josh::housekeeping::discover_filter_candidates(&repo)?;
        josh::housekeeping::refresh_known_filters(
            &repo,
            &known_filters,
            forward_maps.clone(),
            backward_maps.clone(),
        )?;
        return Ok(0);
    }

    let remote = ARGS.value_of("remote").expect("missing remote host url");

    let port = ARGS.value_of("port").unwrap_or("8000").to_owned();
    println!("Now listening on localhost:{}", port);
    let addr = format!("0.0.0.0:{}", port).parse()?;
    let mut core = tokio_core::reactor::Core::new()?;
    let service = run_http_server(
        &mut core,
        addr,
        port,
        &local,
        remote,
        forward_maps.clone(),
        backward_maps.clone(),
    )?;

    josh::housekeeping::spawn_thread(
        local.clone(),
        service.forward_maps.clone(),
        service.backward_maps.clone(),
        ARGS.is_present("gc"),
    );

    /* if ARGS.is_present("g") { */
    /*     josh_proxy::gerrit::spawn_poll_thread(local, remote.to_string()); */
    /* } */

    core.run(futures::future::empty::<(), josh::JoshError>())?;

    Ok(0)
}

fn run_http_server(
    core: &mut tokio_core::reactor::Core,
    addr: std::net::SocketAddr,
    port: String,
    local: &std::path::Path,
    remote: &str,
    forward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
) -> josh::JoshResult<JoshProxyService> {
    let service = JoshProxyService {
        handle: core.handle(),
        /* gerrit: Arc::new(josh_proxy::gerrit::Gerrit::new( */
        /*     &core, */
        /*     local.to_owned(), */
        /*     remote.to_owned(), */
        /* )), */
        fetch_push_pool: futures_cpupool::CpuPool::new(
            ARGS.value_of("n")
                .unwrap_or("1")
                .parse()
                .expect("not a number"),
        ),
        compute_pool: futures_cpupool::CpuPool::new(4),
        port: port,
        repo_path: local.to_owned(),
        forward_maps: forward_maps,
        backward_maps: backward_maps,
        upstream_url: remote.to_owned(),
        credential_cache: Arc::new(RwLock::new(CredentialCache::new())),
        fetching: Arc::new(RwLock::new(std::collections::HashSet::new())),
    };

    let service2 = service.clone();

    let server_handle = service.handle.clone();
    let serve = hyper::server::Http::new().serve_addr_handle(
        &addr,
        &server_handle,
        move || Ok(service2.clone()),
    )?;

    let h2 = server_handle.clone();
    server_handle.spawn(
        serve
            .for_each(move |conn| {
                h2.spawn(
                    conn.map(|_| ()).map_err(|err| {
                        tracing::warn!("serve error:: {:?}", err)
                    }),
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
        repo_update.insert(name.to_string(), std::env::var(&env_name)?);
    }

    repo_update.insert(
        "GIT_DIR".to_owned(),
        git2::Repository::init_bare(&std::path::Path::new(&std::env::var(
            "GIT_DIR",
        )?))?
        .path()
        .to_str()
        .ok_or(josh::josh_error("GIT_DIR not set"))?
        .to_owned(),
    );

    let port = std::env::var("JOSH_PORT")?;

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
            tracing::warn!("/repo_update request failed {:?}", err);
        }
    };
    return Ok(1);
}

fn main() {
    if let [a0, a1, a2, a3, ..] =
        &std::env::args().collect::<Vec<_>>().as_slice()
    {
        if a0.ends_with("/update") {
            println!("josh-proxy");
            std::process::exit(update_hook(&a1, &a2, &a3).unwrap_or(1));
        }
    }

    std::process::exit(run_proxy().unwrap_or(1));
}
