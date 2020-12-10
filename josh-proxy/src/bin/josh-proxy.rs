#[macro_use]
extern crate lazy_static;
use base64;

use tracing_subscriber::layer::SubscriberExt;

use futures::future;
use futures::FutureExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::process::Command;
use tracing_futures::Instrument;

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
    FilteredRepoUrl,
    r"(?P<upstream_repo>/[^:!]*[.]git)(?P<headref>@[^:!]*)?((?P<filter>[:!].*)[.]git)?(?P<pathinfo>/.*)?",
    [upstream_repo, filter, pathinfo, headref]
);

type CredentialCache = HashMap<String, std::time::Instant>;

#[derive(Clone)]
struct JoshProxyService {
    port: String,
    repo_path: std::path::PathBuf,
    /* gerrit: Arc<josh_proxy::gerrit::Gerrit>, */
    upstream_url: String,
    forward_maps: Arc<RwLock<josh::filter_cache::FilterCache>>,
    backward_maps: Arc<RwLock<josh::filter_cache::FilterCache>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    fetch_permits: Arc<tokio::sync::Semaphore>,
    filter_permits: Arc<tokio::sync::Semaphore>,
    credential_store: Arc<RwLock<josh_proxy::CredentialStore>>,
}

impl std::fmt::Debug for JoshProxyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("upstream_url", &self.upstream_url)
            .finish()
    }
}

pub fn parse_auth(
    credential_store: Arc<RwLock<josh_proxy::CredentialStore>>,
    req: hyper::Request<hyper::Body>,
) -> josh::JoshResult<(
    (String, josh_proxy::HashedPassword),
    hyper::Request<hyper::Body>,
)> {
    let blank = |r| {
        Ok((
            (
                "".to_owned(),
                josh_proxy::HashedPassword {
                    hash: "".to_owned(),
                },
            ),
            r,
        ))
    };
    let mut req = req;
    let line = josh::some_or!(
        req.headers_mut()
            .remove("authorization")
            .and_then(|h| Some(h.as_bytes().to_owned())),
        {
            return blank(req);
        }
    );
    let u = josh::ok_or!(String::from_utf8(line[6..].to_vec()), {
        return blank(req);
    });
    let decoded = josh::ok_or!(base64::decode(&u), {
        return blank(req);
    });
    let s = josh::ok_or!(String::from_utf8(decoded), {
        return blank(req);
    });
    if let [username, password] =
        s.as_str().split(':').collect::<Vec<_>>().as_slice()
    {
        use crypto::digest::Digest;
        let mut d = crypto::sha1::Sha1::new();
        d.input_str(&format!("{}:{}", &username, &password));
        let hp = josh_proxy::HashedPassword {
            hash: d.result_str().to_owned(),
        };
        let p = josh_proxy::Password {
            value: password.to_string(),
        };
        credential_store.write()?.insert(hp.clone(), p);
        return Ok(((username.to_string(), hp), req));
    }
    return blank(req);
}

fn hash_strings(url: &str, username: &str, password: &str) -> String {
    use crypto::digest::Digest;
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

#[tracing::instrument]
async fn fetch_upstream(
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    username: &str,
    password: josh_proxy::HashedPassword,
    remote_url: String,
    headref: &str,
) -> josh::JoshResult<bool> {
    let repo = git2::Repository::init_bare(&service.repo_path)?;
    let username = username.to_owned();
    let credentials_hashed =
        hash_strings(&remote_url, &username, &password.hash);

    tracing::debug!(
        "credentials_hashed {:?}, {:?}, {:?}",
        &remote_url,
        &username,
        &credentials_hashed
    );

    let refs_to_fetch = if headref != "" && !headref.starts_with("refs/heads/") {
        vec!["refs/heads/*", "refs/tags/*", headref]
    } else {
        vec!["refs/heads/*", "refs/tags/*"]
    };

    let refs_to_fetch: Vec<_> =
        refs_to_fetch.iter().map(|x| x.to_string()).collect();

    let credentials_cached_ok = {
        if let Some(last) =
            service.credential_cache.read()?.get(&credentials_hashed)
        {
            let since = std::time::Instant::now().duration_since(*last);
            tracing::trace!("last: {:?}, since: {:?}", last, since);
            since < std::time::Duration::from_secs(60)
        } else {
            false
        }
    };

    tracing::trace!("credentials_cached_ok {:?}", credentials_cached_ok);

    if credentials_cached_ok {
        let refname = format!(
            "refs/josh/upstream/{}/{}",
            &josh::to_ns(&upstream_repo),
            headref
        );
        let id = repo.refname_to_id(&refname);
        tracing::trace!("refname_to_id: {:?}", id);
        if id.is_ok() {
            return Ok(true);
        }
    }

    let credential_cache = service.credential_cache.clone();
    let credential_store = service.credential_store.clone();
    let br_path = service.repo_path.clone();

    let permit = service.fetch_permits.acquire().await;

    let s = tracing::span!(tracing::Level::TRACE, "fetch worker");
    let res = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        josh_proxy::fetch_refs_from_url(
            &br_path,
            &upstream_repo,
            &remote_url,
            &refs_to_fetch,
            &username,
            &password,
            credential_store,
        )
    })
    .await??;

    std::mem::drop(permit);

    if res {
        credential_cache
            .write()?
            .insert(credentials_hashed, std::time::Instant::now());
    }

    return Ok(res);
}

async fn static_paths(
    service: &JoshProxyService,
    path: &str,
) -> josh::JoshResult<Option<Response<hyper::Body>>> {
    if path == "/version" {
        return Ok(Some(
            Response::builder()
                .status(hyper::StatusCode::OK)
                .body(hyper::Body::from(version_str()))
                .unwrap_or(Response::default()),
        ));
    }
    if path == "/flush" {
        service.credential_cache.write()?.clear();
        return Ok(Some(
            Response::builder()
                .status(hyper::StatusCode::OK)
                .body(hyper::Body::from("Flushed credential cache\n"))
                .unwrap_or(Response::default()),
        ));
    }
    if path == "/filters" {
        service.credential_cache.write()?.clear();
        let service = service.clone();

        let body_str =
            tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
                let repo = git2::Repository::init_bare(&service.repo_path)?;
                let known_filters =
                    josh::housekeeping::discover_filter_candidates(&repo).ok();
                Ok(toml::to_string_pretty(&known_filters)?)
            })
            .await??;

        return Ok(Some(
            Response::builder()
                .status(hyper::StatusCode::OK)
                .body(hyper::Body::from(body_str))
                .unwrap_or(Response::default()),
        ));
    }
    return Ok(None);
}

#[tracing::instrument]
async fn repo_update_fn(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
) -> josh::JoshResult<Response<hyper::Body>> {
    let forward_maps = serv.forward_maps.clone();
    let backward_maps = serv.backward_maps.clone();

    let body = hyper::body::to_bytes(req.into_body()).await;

    let s = tracing::span!(tracing::Level::TRACE, "repo update worker");
    let result = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        let body = body?;
        let buffer = std::str::from_utf8(&body)?;
        josh_proxy::process_repo_update(
            serv.credential_store.clone(),
            serde_json::from_str(&buffer)?,
            forward_maps,
            backward_maps,
        )
    })
    .await?;

    return Ok(match result {
        Ok(stderr) => Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(stderr)),
        Err(josh::JoshError(stderr)) => Response::builder()
            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
            .body(hyper::Body::from(stderr)),
    }?);
}

#[tracing::instrument]
async fn do_filter(
    repo_path: std::path::PathBuf,
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    temp_ns: Arc<josh_proxy::TmpGitNamespace>,
    filter_spec: String,
    headref: String,
) -> josh::JoshResult<git2::Repository> {
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();
    let permit = service.filter_permits.acquire().await;

    let s = tracing::span!(tracing::Level::TRACE, "do_filter worker");
    let r = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        tracing::trace!("in do_filter worker");
        let repo = git2::Repository::init_bare(&repo_path)?;
        let filter = josh::filters::parse(&filter_spec);
        let filter_spec = filter.filter_spec();
        let mut from_to = josh::housekeeping::default_from_to(
            &repo,
            &temp_ns.name(),
            &upstream_repo,
            &filter_spec,
        );

        from_to.push((
            format!(
                "refs/josh/upstream/{}/{}",
                &josh::to_ns(&upstream_repo),
                headref
            ),
            temp_ns.reference(&headref),
        ));

        let mut bm = josh::filter_cache::new_downstream(&backward_maps);
        let mut fm = josh::filter_cache::new_downstream(&forward_maps);
        josh::scratch::apply_filter_to_refs(
            &repo, &*filter, &from_to, &mut fm, &mut bm,
        )?;
        josh::filter_cache::try_merge_both(
            forward_maps,
            backward_maps,
            &fm,
            &bm,
        );
        repo.reference_symbolic(
            &temp_ns.reference("HEAD"),
            &temp_ns.reference(&headref),
            true,
            "",
        )?;
        return Ok(repo);
    })
    .await?;

    std::mem::drop(permit);

    return r;
}

async fn error_response() -> Response<hyper::Body> {
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(hyper::Body::empty())
        .expect("Can't build response")
}

#[tracing::instrument]
async fn call_service(
    serv: Arc<JoshProxyService>,
    req_auth: ((String, josh_proxy::HashedPassword), Request<hyper::Body>),
) -> josh::JoshResult<Response<hyper::Body>> {
    let (auth, req) = req_auth;

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if let Some(r) = static_paths(&serv, &path).await? {
        return Ok(r);
    }

    if path == "/repo_update" {
        return repo_update_fn(serv, req).await;
    }

    let parsed_url = {
        if let Some(parsed_url) = FilteredRepoUrl::from_str(&path) {
            let mut pu = parsed_url;
            if pu.filter == "" {
                pu.filter = ":nop".to_string();
            }
            pu
        } else {
            return Ok(Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .body(hyper::Body::empty())?);
        }
    };

    let mut headref = parsed_url.headref.trim_start_matches("@").to_owned();
    if headref == "" {
        headref = "refs/heads/master".to_string();
    }

    let remote_url = [
        serv.upstream_url.as_str(),
        parsed_url.upstream_repo.as_str(),
    ]
    .join("");

    let (username, password) = auth;

    let temp_ns = match prepare_namespace(
        serv.clone(),
        &parsed_url.upstream_repo,
        &remote_url,
        &parsed_url.filter,
        &headref,
        (username.clone(), password.clone()),
    )
    .in_current_span()
    .await?
    {
        PrepareNsResult::Ns(temp_ns) => temp_ns,
        PrepareNsResult::Resp(resp) => return Ok(resp),
    };

    if req.uri().query() == Some("info") {
        let forward_maps = serv.forward_maps.clone();
        let backward_maps = serv.forward_maps.clone();

        let info_str =
            tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
                let repo = git2::Repository::init_bare(&serv.repo_path)?;
                josh::housekeeping::get_info(
                    &repo,
                    &*josh::filters::parse(&parsed_url.filter),
                    &parsed_url.upstream_repo,
                    &headref,
                    forward_maps.clone(),
                    backward_maps.clone(),
                )
            })
            .await??;

        return Ok(Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(format!("{}\n", info_str)))?);
    }

    let repo_path = serv
        .repo_path
        .to_str()
        .ok_or(josh::josh_error("repo_path.to_str"))?;

    if let Some(q) = req.uri().query().map(|x| x.to_string()) {
        if parsed_url.pathinfo == "" {
            let s = tracing::span!(tracing::Level::TRACE, "render worker");
            let res =
                tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
                    let _e = s.enter();
                    let repo = git2::Repository::init_bare(&serv.repo_path)?;
                    josh::query::render(
                        repo,
                        &temp_ns.reference(&headref),
                        &q,
                        josh::filter_cache::new_downstream(&serv.forward_maps),
                        josh::filter_cache::new_downstream(&serv.backward_maps),
                    )
                })
                .await??;
            if let Some(res) = res {
                return Ok(Response::builder()
                    .status(hyper::StatusCode::OK)
                    .body(hyper::Body::from(res))?);
            } else {
                return Ok(Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(hyper::Body::from("File not found".to_string()))?);
            }
        }
    }

    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&serv.repo_path);
    cmd.env("GIT_DIR", repo_path);
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.env("GIT_NAMESPACE", temp_ns.name().clone());
    cmd.env("GIT_PROJECT_ROOT", repo_path);
    cmd.env("JOSH_BASE_NS", josh::to_ns(&parsed_url.upstream_repo));
    cmd.env("JOSH_PASSWORD", password.hash);
    cmd.env("JOSH_PORT", serv.port.clone());
    cmd.env("JOSH_REMOTE", remote_url);
    cmd.env("JOSH_USERNAME", username);
    cmd.env("JOSH_VIEWSTR", parsed_url.filter.clone());
    cmd.env("PATH_INFO", parsed_url.pathinfo.clone());

    let cgires = hyper_cgi::do_cgi(req, cmd)
        .instrument(tracing::span!(tracing::Level::TRACE, "git http-backend"))
        .await
        .0;

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    std::mem::drop(temp_ns);

    return Ok(cgires);
}

enum PrepareNsResult {
    Ns(std::sync::Arc<josh_proxy::TmpGitNamespace>),
    Resp(hyper::Response<hyper::Body>),
}

#[tracing::instrument]
async fn prepare_namespace(
    serv: Arc<JoshProxyService>,
    upstream_repo: &str,
    remote_url: &str,
    filter_spec: &str,
    headref: &str,
    auth: (String, josh_proxy::HashedPassword),
) -> josh::JoshResult<PrepareNsResult> {
    let (username, password) = auth;

    if ARGS.is_present("require-auth") && username == "" {
        tracing::trace!("require-auth");
        let builder = Response::builder()
            .header("WWW-Authenticate", "Basic realm=User Visible Realm")
            .status(hyper::StatusCode::UNAUTHORIZED);
        return Ok(PrepareNsResult::Resp(builder.body(hyper::Body::empty())?));
    }

    let authorized = fetch_upstream(
        serv.clone(),
        upstream_repo.to_owned(),
        &username,
        password.clone(),
        remote_url.to_owned(),
        &headref,
    )
    .await?;

    if !authorized {
        let builder = Response::builder()
            .header("WWW-Authenticate", "Basic realm=User Visible Realm")
            .status(hyper::StatusCode::UNAUTHORIZED);
        return Ok(PrepareNsResult::Resp(builder.body(hyper::Body::empty())?));
    }

    let temp_ns = Arc::new(josh_proxy::TmpGitNamespace::new(
        &serv.repo_path,
        tracing::Span::current(),
    ));

    let serv = serv.clone();

    do_filter(
        serv.repo_path.clone(),
        serv.clone(),
        upstream_repo.to_owned(),
        temp_ns.to_owned(),
        filter_spec.to_owned(),
        headref.to_string(),
    )
    .await?;

    return Ok(PrepareNsResult::Ns(temp_ns));
}

#[tokio::main]
async fn run_proxy() -> josh::JoshResult<i32> {
    let port = ARGS.value_of("port").unwrap_or("8000").to_owned();
    let addr = format!("0.0.0.0:{}", port).parse()?;

    let remote = ARGS
        .value_of("remote")
        .ok_or(josh::josh_error("missing remote host url"))?;
    let local = std::path::PathBuf::from(
        ARGS.value_of("local")
            .ok_or(josh::josh_error("missing local directory"))?,
    );

    josh_proxy::create_repo(&local)?;

    let forward_maps = Arc::new(RwLock::new(josh::filter_cache::try_load(
        &local.join("josh_forward_maps"),
    )));
    let backward_maps = Arc::new(RwLock::new(josh::filter_cache::try_load(
        &local.join("josh_backward_maps"),
    )));

    let proxy_service = Arc::new(JoshProxyService {
        port: port,
        repo_path: local.to_owned(),
        forward_maps: forward_maps.clone(),
        backward_maps: backward_maps.clone(),
        upstream_url: remote.to_owned(),
        credential_cache: Arc::new(RwLock::new(CredentialCache::new())),
        fetch_permits: Arc::new(tokio::sync::Semaphore::new(
            ARGS.value_of("n").unwrap_or("1").parse()?,
        )),
        filter_permits: Arc::new(tokio::sync::Semaphore::new(10)),
        credential_store: Arc::new(RwLock::new(HashMap::new())),
    });

    let make_service = make_service_fn(move |_| {
        let proxy_service = proxy_service.clone();

        let service = service_fn(move |_req| {
            let proxy_service = proxy_service.clone();

            async {
                if let Ok(req_auth) =
                    parse_auth(proxy_service.credential_store.clone(), _req)
                {
                    if let Ok(r) = call_service(proxy_service, req_auth).await {
                        r
                    } else {
                        error_response().await
                    }
                } else {
                    error_response().await
                }
            }
            .map(Ok::<_, hyper::http::Error>)
        });

        future::ok::<_, hyper::http::Error>(service)
    });

    let server = Server::bind(&addr).serve(make_service);

    let _jh = josh::housekeeping::spawn_thread(
        local,
        forward_maps,
        backward_maps,
        ARGS.is_present("gc"),
    );
    println!("Now listening on {}", addr);

    server.with_graceful_shutdown(shutdown_signal()).await?;
    Ok(0)
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
            clap::Arg::with_name("gc")
                .long("gc")
                .takes_value(false)
                .help("Run git gc in maintanance"),
        )
        .arg(
            clap::Arg::with_name("require-auth")
                .long("require-auth")
                .takes_value(false),
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

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let resp = client
        .post(&format!("http://localhost:{}/repo_update", port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(r) => {
            let success = r.status().is_success();
            if let Ok(body) = r.text() {
                println!("response from upstream:\n {}\n\n", body);
            } else {
                println!("no upstream response");
            }
            if success {
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

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    println!("shutdown_signal");
}

fn main() {
    // josh-proxy creates a symlink to itself as a git update hook.
    // When it gets called by git as that hook, the binary name will end
    // end in "/update" and this will not be a new server.
    // The update hook will then make a http request back to the main
    // process to do the actual computation while taking advantage of the
    // cached data already loaded into the main processe's memory.
    if let [a0, a1, a2, a3, ..] =
        &std::env::args().collect::<Vec<_>>().as_slice()
    {
        if a0.ends_with("/update") {
            println!("josh-proxy");
            std::process::exit(update_hook(&a1, &a2, &a3).unwrap_or(1));
        }
    }

    let fmt_layer = tracing_subscriber::fmt::layer().with_ansi(false);

    let (tracer, _uninstall) = opentelemetry_jaeger::new_pipeline()
        .with_service_name(
            std::env::var("JOSH_SERVICE_NAME")
                .unwrap_or("josh-proxy".to_owned()),
        )
        .with_agent_endpoint(
            std::env::var("JOSH_JAEGER_ENDPOINT")
                .unwrap_or("localhost:6831".to_owned()),
        )
        .install()
        .expect("can't install opentelemetry pipeline");

    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = tracing_subscriber::Registry::default()
        .with(telemetry_layer)
        .with(fmt_layer);

    tracing::subscriber::set_global_default(subscriber)
        .expect("can't set_global_default");

    let local = std::path::PathBuf::from(
        ARGS.value_of("local").expect("missing local directory"),
    );
    if ARGS.is_present("m") {
        let forward_maps = Arc::new(RwLock::new(josh::filter_cache::try_load(
            &local.join("josh_forward_maps"),
        )));
        let backward_maps = Arc::new(RwLock::new(
            josh::filter_cache::try_load(&local.join("josh_backward_maps")),
        ));
        let repo =
            git2::Repository::init_bare(&local).expect("can't init_bare");
        let known_filters =
            josh::housekeeping::discover_filter_candidates(&repo)
                .expect("can't discover_filter_candidates");
        josh::housekeeping::refresh_known_filters(
            &repo,
            &known_filters,
            forward_maps,
            backward_maps,
        )
        .expect("can't refresh_known_filters");
        std::process::exit(0);
    }

    std::process::exit(run_proxy().unwrap_or(1));
}
