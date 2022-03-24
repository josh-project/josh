#![deny(warnings)]
#[macro_use]
extern crate lazy_static;

use tracing_subscriber::Layer;

use futures::future;
use futures::FutureExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server, StatusCode};
use indoc::formatdoc;
use josh::JoshError;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::process::Command;
use tracing::Span;
use tracing_futures::Instrument;

fn version_str() -> String {
    format!(
        "Version: {}\n",
        option_env!("GIT_DESCRIBE").unwrap_or(std::env!("CARGO_PKG_VERSION"))
    )
}

lazy_static! {
    static ref ARGS: clap::ArgMatches = parse_args();
}

josh::regex_parsed!(
    FilteredRepoUrl,
    r"(?P<api>/~/\w+)?(?P<upstream_repo>/[^:!]*[.]git)(?P<headref>@[^:!]*)?((?P<filter>[:!].*)[.]git)?(?P<pathinfo>/.*)?(?P<rest>.*)",
    [api, upstream_repo, filter, pathinfo, headref, rest]
);

type FetchTimers = HashMap<String, std::time::Instant>;
type Polls =
    Arc<std::sync::Mutex<std::collections::HashSet<(String, josh_proxy::auth::Handle, String)>>>;

type HeadsMap = Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>;

#[derive(Clone)]
struct JoshProxyService {
    port: String,
    repo_path: std::path::PathBuf,
    upstream_url: String,
    fetch_timers: Arc<RwLock<FetchTimers>>,
    heads_map: HeadsMap,
    fetch_permits: Arc<tokio::sync::Semaphore>,
    filter_permits: Arc<tokio::sync::Semaphore>,
    poll: Polls,
}

impl std::fmt::Debug for JoshProxyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("upstream_url", &self.upstream_url)
            .finish()
    }
}

#[tracing::instrument]
async fn fetch_upstream(
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    auth: &josh_proxy::auth::Handle,
    remote_url: String,
    headref: &str,
    force: bool,
) -> josh::JoshResult<bool> {
    let auth = auth.clone();
    let key = remote_url.clone();

    let refs_to_fetch =
        if !headref.is_empty() && headref != "HEAD" && !headref.starts_with("refs/heads/") {
            vec!["HEAD*", "refs/heads/*", "refs/tags/*", headref]
        } else {
            vec!["HEAD*", "refs/heads/*", "refs/tags/*"]
        };

    let refs_to_fetch: Vec<_> = refs_to_fetch.iter().map(|x| x.to_string()).collect();

    let fetch_cached_ok = {
        if let Some(last) = service.fetch_timers.read()?.get(&key) {
            let since = std::time::Instant::now().duration_since(*last);
            let max = std::time::Duration::from_secs(
                ARGS.value_of("cache-duration").unwrap_or("0").parse()?,
            );

            tracing::trace!("last: {:?}, since: {:?}, max: {:?}", last, since, max);
            since < max
        } else {
            false
        }
    };

    let fetch_cached_ok = fetch_cached_ok && !force;

    tracing::trace!("fetch_cached_ok {:?}", fetch_cached_ok);

    if fetch_cached_ok && headref.is_empty() {
        return Ok(true);
    }

    if fetch_cached_ok && !headref.is_empty() {
        let transaction = josh::cache::Transaction::open(
            &service.repo_path,
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&upstream_repo),
            )),
        )?;
        let id = transaction
            .repo()
            .refname_to_id(&transaction.refname(headref));
        tracing::trace!("refname_to_id: {:?}", id);
        if id.is_ok() {
            return Ok(true);
        }
    }

    let fetch_timers = service.fetch_timers.clone();
    let heads_map = service.heads_map.clone();
    let br_path = service.repo_path.clone();

    let s = tracing::span!(tracing::Level::TRACE, "fetch worker");
    let us = upstream_repo.clone();
    let a = auth.clone();
    let ru = remote_url.clone();
    let permit = service.fetch_permits.acquire().await;
    let res = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        josh_proxy::fetch_refs_from_url(&br_path, &us, &ru, &refs_to_fetch, &a)
    })
    .await?;

    let us = upstream_repo.clone();
    let s = tracing::span!(tracing::Level::TRACE, "get_head worker");
    let br_path = service.repo_path.clone();
    let ru = remote_url.clone();
    let a = auth.clone();
    let hres = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        josh_proxy::get_head(&br_path, &ru, &a)
    })
    .await?;

    if let Ok(hres) = hres {
        heads_map.write()?.insert(us, hres);
    }

    std::mem::drop(permit);

    if let Ok(res) = res {
        if res {
            fetch_timers.write()?.insert(key, std::time::Instant::now());

            if ARGS.value_of("poll") == Some(&auth.parse()?.0) {
                service
                    .poll
                    .lock()?
                    .insert((upstream_repo, auth, remote_url));
            }
        }
        return Ok(res);
    }
    res
}

async fn static_paths(
    service: &JoshProxyService,
    path: &str,
) -> josh::JoshResult<Option<Response<hyper::Body>>> {
    tracing::debug!("static_path {:?}", path);
    if path == "/version" {
        return Ok(Some(make_response(
            hyper::Body::from(version_str()),
            hyper::StatusCode::OK,
        )));
    }
    if path == "/remote" {
        return Ok(Some(make_response(
            hyper::Body::from(service.upstream_url.clone()),
            hyper::StatusCode::OK,
        )));
    }
    if path == "/flush" {
        service.fetch_timers.write()?.clear();
        return Ok(Some(make_response(
            hyper::Body::from("Flushed credential cache\n"),
            hyper::StatusCode::OK,
        )));
    }
    if path == "/filters" || path == "/filters/refresh" {
        service.fetch_timers.write()?.clear();
        let service = service.clone();
        let refresh = path == "/filters/refresh";

        let body_str = tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
            let transaction = josh::cache::Transaction::open(&service.repo_path, None)?;
            let known_filters = josh::housekeeping::discover_filter_candidates(&transaction)?;
            if refresh {
                josh::housekeeping::refresh_known_filters(&transaction, &known_filters)?;
            }
            Ok(toml::to_string_pretty(&known_filters)?)
        })
        .await??;

        return Ok(Some(make_response(
            hyper::Body::from(body_str),
            hyper::StatusCode::OK,
        )));
    }
    Ok(None)
}

#[tracing::instrument]
async fn repo_update_fn(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
) -> josh::JoshResult<Response<hyper::Body>> {
    let body = hyper::body::to_bytes(req.into_body()).await;

    let s = tracing::span!(tracing::Level::TRACE, "repo update worker");
    let result = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        let body = body?;
        let buffer = std::str::from_utf8(&body)?;
        josh_proxy::process_repo_update(serde_json::from_str(buffer)?)
    })
    .await?;

    Ok(match result {
        Ok(stderr) => Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(stderr)),
        Err(josh::JoshError(stderr)) => Response::builder()
            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
            .body(hyper::Body::from(stderr)),
    }?)
}

#[tracing::instrument]
async fn do_filter(
    repo_path: std::path::PathBuf,
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    temp_ns: Arc<josh_proxy::TmpGitNamespace>,
    filter_spec: String,
    headref: String,
) -> josh::JoshResult<()> {
    let permit = service.filter_permits.acquire().await;
    let heads_map = service.heads_map.clone();

    let s = tracing::span!(tracing::Level::TRACE, "do_filter worker");
    let r = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        tracing::trace!("in do_filter worker");
        let transaction = josh::cache::Transaction::open(
            &repo_path,
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&upstream_repo),
            )),
        )?;
        let filter = josh::filter::parse(&filter_spec)?;
        let filter_spec = josh::filter::spec(filter);
        let mut from_to = josh::housekeeping::default_from_to(
            transaction.repo(),
            temp_ns.name(),
            &upstream_repo,
            &filter_spec,
        );

        let glob = format!(
            "refs/josh/rewrites/{}/{:?}/r_*",
            josh::to_ns(&upstream_repo),
            filter.id()
        );
        for reference in transaction.repo().references_glob(&glob).unwrap() {
            let reference = reference.unwrap();
            let refname = reference.name().unwrap();
            transaction.repo().reference(
                &temp_ns.reference(refname),
                reference.target().unwrap(),
                true,
                "rewrite",
            )?;
        }

        from_to.push((
            format!(
                "refs/josh/upstream/{}/{}",
                &josh::to_ns(&upstream_repo),
                headref
            ),
            temp_ns.reference(&headref),
        ));

        let mut headref = headref;

        josh::filter_refs(&transaction, filter, &from_to, josh::filter::empty())?;
        if headref == "HEAD" {
            headref = heads_map
                .read()?
                .get(&upstream_repo)
                .unwrap_or(&"invalid".to_string())
                .clone();
        }
        transaction
            .repo()
            .reference_symbolic(
                &temp_ns.reference("HEAD"),
                &temp_ns.reference(&headref),
                true,
                "",
            )
            .ok();
        Ok(())
    })
    .await?;

    std::mem::drop(permit);

    r
}

fn make_response(body: hyper::Body, code: hyper::StatusCode) -> Response<hyper::Body> {
    Response::builder()
        .status(code)
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .body(body)
        .expect("Can't build response")
}

#[tracing::instrument]
async fn call_service(
    serv: Arc<JoshProxyService>,
    req_auth: (josh_proxy::auth::Handle, Request<hyper::Body>),
) -> josh::JoshResult<Response<hyper::Body>> {
    let (auth, req) = req_auth;
    let (username, _) = auth.parse()?;

    tracing::event!(tracing::Level::TRACE, username = username.as_str());

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if ARGS.is_present("graphql-root") {
        if path == "/~/graphiql" {
            return Ok(tokio::task::spawn_blocking(move || {
                josh_proxy::juniper_hyper::graphiql("/~/graphql", None)
            })
            .await??);
        }

        if path == "/~/graphql" {
            let ctx = std::sync::Arc::new(josh::graphql::context(josh::cache::Transaction::open(
                &serv.repo_path,
                None,
            )?));
            let root_node = std::sync::Arc::new(josh::graphql::schema());
            return Ok(josh_proxy::juniper_hyper::graphql(root_node, ctx, req).await?);
        }
    }

    if path.starts_with("/~/select")
        || path.starts_with("/~/browse")
        || path.starts_with("/~/filter")
        || path.starts_with("/~/refs")
    {
        let p = &path[9..];

        let result = hyper_staticfile::resolve_path("static", p).await?;
        let result = if let hyper_staticfile::ResolveResult::NotFound = result {
            hyper_staticfile::resolve_path("static", "index.html").await?
        } else {
            result
        };

        let r = hyper_staticfile::ResponseBuilder::new()
            .request(&req)
            .build(result)?;

        return Ok(r);
    }

    if let Some(r) = static_paths(&serv, &path).await? {
        return Ok(r);
    }

    if path == "/repo_update" {
        return repo_update_fn(serv, req).await;
    }

    let parsed_url = {
        if let Some(parsed_url) = FilteredRepoUrl::from_str(&path) {
            let mut pu = parsed_url;

            if pu.rest.starts_with(":") {
                let guessed_url = path.trim_end_matches("/info/refs");
                return Ok(make_response(
                    hyper::Body::from(formatdoc!(
                        r#"
                        Invalid URL: "{0}"

                        Note: repository URLs should end with ".git":

                          {0}.git
                        "#,
                        guessed_url
                    )),
                    hyper::StatusCode::UNPROCESSABLE_ENTITY,
                ));
            }

            if pu.filter.is_empty() {
                pu.filter = ":/".to_string();
            }
            pu
        } else {
            let redirect_path = if path == "/" {
                "/~/select/@()/()".to_string()
            } else {
                format!("/~/browse{}@HEAD(:/)/()", path)
            };

            return Ok(Response::builder()
                .status(hyper::StatusCode::FOUND)
                .header("Location", redirect_path)
                .body(hyper::Body::empty())?);
        }
    };

    let remote_url = [
        serv.upstream_url.as_str(),
        parsed_url.upstream_repo.as_str(),
    ]
    .join("");

    if parsed_url.pathinfo.starts_with("/info/lfs") {
        return Ok(Response::builder()
            .status(307)
            .header("Location", format!("{}{}", remote_url, parsed_url.pathinfo))
            .body(hyper::Body::empty())?);
    }

    let mut headref = parsed_url.headref.trim_start_matches('@').to_owned();
    if headref.is_empty() {
        headref = "HEAD".to_string();
    }

    if !josh_proxy::auth::check_auth(&remote_url, &auth, ARGS.is_present("require-auth"))
        .in_current_span()
        .await?
    {
        tracing::trace!("require-auth");
        let builder = Response::builder()
            .header("WWW-Authenticate", "Basic realm=User Visible Realm")
            .status(hyper::StatusCode::UNAUTHORIZED);
        return Ok(builder.body(hyper::Body::empty())?);
    }

    let block = std::env::var("JOSH_REPO_BLOCK").unwrap_or("".to_owned());
    let block = block.split(";").collect::<Vec<_>>();

    for b in block {
        if b == parsed_url.upstream_repo {
            return Ok(make_response(
                hyper::Body::from(formatdoc!(
                    r#"
                    Access to this repo is blocked via JOSH_REPO_BLOCK
                    "#
                )),
                hyper::StatusCode::UNPROCESSABLE_ENTITY,
            ));
        }
    }

    match fetch_upstream(
        serv.clone(),
        parsed_url.upstream_repo.to_owned(),
        &auth,
        remote_url.to_owned(),
        &headref,
        false,
    )
    .in_current_span()
    .await
    {
        Ok(res) => {
            if !res {
                let builder = Response::builder()
                    .header("WWW-Authenticate", "Basic realm=User Visible Realm")
                    .status(hyper::StatusCode::UNAUTHORIZED);
                return Ok(builder.body(hyper::Body::empty())?);
            }
        }
        Err(res) => {
            let builder = Response::builder()
                .header("WWW-Authenticate", "Basic realm=User Visible Realm")
                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR);
            return Ok(builder.body(hyper::Body::from(res.0))?);
        }
    }

    if parsed_url.api == "/~/graphiql" {
        let addr = format!("/~/graphql{}", parsed_url.upstream_repo);
        return Ok(tokio::task::spawn_blocking(move || {
            josh_proxy::juniper_hyper::graphiql(&addr, None)
        })
        .in_current_span()
        .await??);
    }

    if parsed_url.api == "/~/graphql" {
        let ctx = std::sync::Arc::new(josh::graphql::context(josh::cache::Transaction::open(
            &serv.repo_path,
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&parsed_url.upstream_repo),
            )),
        )?));
        let root_node = std::sync::Arc::new(josh::graphql::repo_schema(
            parsed_url
                .upstream_repo
                .strip_suffix(".git")
                .unwrap_or(&parsed_url.upstream_repo)
                .to_string(),
            false,
        ));
        return Ok(josh_proxy::juniper_hyper::graphql(root_node, ctx, req)
            .in_current_span()
            .await?);
    }

    if req.uri().query() == Some("info") {
        let info_str = tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
            let transaction = josh::cache::Transaction::open(
                &serv.repo_path,
                Some(&format!(
                    "refs/josh/upstream/{}/",
                    &josh::to_ns(&parsed_url.upstream_repo),
                )),
            )?;
            josh::housekeeping::get_info(
                &transaction,
                josh::filter::parse(&parsed_url.filter)?,
                &headref,
            )
        })
        .in_current_span()
        .await??;

        return Ok(Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(format!("{}\n", info_str)))?);
    }

    let repo_path = serv
        .repo_path
        .to_str()
        .ok_or(josh::josh_error("repo_path.to_str"))?;

    let temp_ns = prepare_namespace(
        serv.clone(),
        &parsed_url.upstream_repo,
        &parsed_url.filter,
        &headref,
    )
    .in_current_span()
    .await?;

    if let Some(q) = req.uri().query().map(|x| x.to_string()) {
        if parsed_url.pathinfo.is_empty() {
            let s = tracing::span!(tracing::Level::TRACE, "render worker");
            let res = tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
                let _e = s.enter();
                let transaction = josh::cache::Transaction::open(
                    &serv.repo_path,
                    Some(&format!(
                        "refs/josh/upstream/{}/",
                        &josh::to_ns(&parsed_url.upstream_repo),
                    )),
                )?;

                josh::query::render(transaction.repo(), "", &temp_ns.reference(&headref), &q)
            })
            .in_current_span()
            .await?;
            match res {
                Ok(res) => {
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
                Err(res) => {
                    return Ok(Response::builder()
                        .status(hyper::StatusCode::UNPROCESSABLE_ENTITY)
                        .body(hyper::Body::from(res.to_string()))?)
                }
            }
        }
    }

    let repo_update = josh_proxy::RepoUpdate {
        refs: HashMap::new(),
        remote_url: remote_url.clone(),
        auth,
        port: serv.port.clone(),
        filter_spec: parsed_url.filter.clone(),
        base_ns: josh::to_ns(&parsed_url.upstream_repo),
        git_ns: temp_ns.name().to_string(),
        git_dir: repo_path.to_string(),
    };

    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&serv.repo_path);
    cmd.env("GIT_DIR", repo_path);
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.env("GIT_NAMESPACE", temp_ns.name().clone());
    cmd.env("GIT_PROJECT_ROOT", repo_path);
    cmd.env("JOSH_REPO_UPDATE", serde_json::to_string(&repo_update)?);
    cmd.env("PATH_INFO", parsed_url.pathinfo.clone());

    let cgires = hyper_cgi::do_cgi(req, cmd)
        .instrument(tracing::span!(tracing::Level::TRACE, "git http-backend"))
        .await
        .0;

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    std::mem::drop(temp_ns);

    Ok(cgires)
}

#[tracing::instrument]
async fn prepare_namespace(
    serv: Arc<JoshProxyService>,
    upstream_repo: &str,
    filter_spec: &str,
    headref: &str,
) -> josh::JoshResult<std::sync::Arc<josh_proxy::TmpGitNamespace>> {
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

    Ok(temp_ns)
}

fn trace_http_response_code(trace_span: Span, http_status: StatusCode) {
    macro_rules! trace {
        ($level:expr) => {{
            tracing::event!(
                parent: trace_span,
                $level,
                http_status = http_status.as_u16()
            );
        }};
    }

    match http_status.as_u16() {
        s if s < 400 => trace!(tracing::Level::TRACE),
        s if s < 500 => trace!(tracing::Level::WARN),
        _ => trace!(tracing::Level::ERROR),
    };
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
    josh::cache::load(&local)?;

    let proxy_service = Arc::new(JoshProxyService {
        port,
        repo_path: local.to_owned(),
        upstream_url: remote.to_owned(),
        fetch_timers: Arc::new(RwLock::new(FetchTimers::new())),
        heads_map: Arc::new(RwLock::new(std::collections::HashMap::new())),
        poll: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        fetch_permits: Arc::new(tokio::sync::Semaphore::new(
            ARGS.value_of("n").unwrap_or("1").parse()?,
        )),
        filter_permits: Arc::new(tokio::sync::Semaphore::new(10)),
    });

    let ps = proxy_service.clone();

    let make_service = make_service_fn(move |_| {
        let proxy_service = proxy_service.clone();

        let service = service_fn(move |_req| {
            let proxy_service = proxy_service.clone();

            let _s = tracing::span!(
                tracing::Level::TRACE,
                "http_request",
                path = _req.uri().path()
            );
            let s = _s;

            async move {
                let r = if let Ok(req_auth) = josh_proxy::auth::strip_auth(_req) {
                    match call_service(proxy_service, req_auth)
                        .instrument(s.clone())
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => make_response(
                            hyper::Body::from(match e {
                                JoshError(s) => s,
                            }),
                            hyper::StatusCode::INTERNAL_SERVER_ERROR,
                        ),
                    }
                } else {
                    make_response(
                        hyper::Body::from("JoshError(strip_auth)"),
                        hyper::StatusCode::INTERNAL_SERVER_ERROR,
                    )
                };
                let _e = s.enter();
                trace_http_response_code(s.clone(), r.status());
                r
            }
            .map(Ok::<_, hyper::http::Error>)
        });

        future::ok::<_, hyper::http::Error>(service)
    });

    let server = Server::bind(&addr).serve(make_service);

    println!("Now listening on {}", addr);

    let server_future = server.with_graceful_shutdown(shutdown_signal());

    if ARGS.is_present("no-background") {
        tokio::select!(
            _ = server_future => println!("http server exited"),
        );
    } else {
        tokio::select!(
            _ = run_housekeeping(local) => println!("run_housekeeping exited"),
            _ = run_polling(ps.clone()) => println!("run_polling exited"),
            _ = server_future => println!("http server exited"),
        );
    }
    Ok(0)
}

async fn run_polling(serv: Arc<JoshProxyService>) -> josh::JoshResult<()> {
    loop {
        let polls = serv.poll.lock()?.clone();

        for (upstream_repo, auth, url) in polls {
            fetch_upstream(
                serv.clone(),
                upstream_repo.clone(),
                &auth,
                url.clone(),
                "",
                true,
            )
            .in_current_span()
            .await?;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

async fn run_housekeeping(local: std::path::PathBuf) -> josh::JoshResult<()> {
    let mut i: usize = 0;
    loop {
        let local = local.clone();
        tokio::task::spawn_blocking(move || {
            josh::housekeeping::run(&local, (i % 60 == 0) && ARGS.is_present("gc"))
        })
        .await??;
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        i += 1;
    }
}

fn make_app() -> clap::App<'static> {
    clap::App::new("josh-proxy")
        .arg(clap::Arg::new("remote").long("remote").takes_value(true))
        .arg(clap::Arg::new("local").long("local").takes_value(true))
        .arg(clap::Arg::new("poll").long("poll").takes_value(true))
        .arg(
            clap::Arg::new("gc")
                .long("gc")
                .takes_value(false)
                .help("Run git gc in maintanance"),
        )
        .arg(
            clap::Arg::new("require-auth")
                .long("require-auth")
                .takes_value(false),
        )
        .arg(
            clap::Arg::new("no-background")
                .long("no-background")
                .takes_value(false),
        )
        .arg(
            clap::Arg::new("graphql-root")
                .long("graphql-root")
                .help("Enable graphql root endpoint (caution: This bypasses authentication!)")
                .takes_value(false),
        )
        .arg(
            clap::Arg::new("n")
                .short('n')
                .takes_value(true)
                .help("Number of concurrent upstream git fetch/push operations"),
        )
        .arg(clap::Arg::new("port").long("port").takes_value(true))
        .arg(
            clap::Arg::new("cache-duration")
                .long("cache-duration")
                .short('c')
                .takes_value(true)
                .help("Duration between forced cache refresh"),
        )
}

fn parse_args() -> clap::ArgMatches {
    let args = {
        let mut args = vec![];
        for arg in std::env::args() {
            args.push(arg);
        }
        args
    };

    make_app().get_matches_from(args)
}

fn pre_receive_hook() -> josh::JoshResult<i32> {
    let repo_update: josh_proxy::RepoUpdate =
        serde_json::from_str(&std::env::var("JOSH_REPO_UPDATE")?)?;

    let p = std::path::PathBuf::from(repo_update.git_dir)
        .join("refs/namespaces")
        .join(repo_update.git_ns)
        .join("push_options");

    let n: usize = std::env::var("GIT_PUSH_OPTION_COUNT")?.parse()?;

    let mut push_options = std::collections::HashMap::<String, String>::new();
    for i in 0..n {
        let s = std::env::var(format!("GIT_PUSH_OPTION_{}", i))?;
        if let [key, value] = s.as_str().split('=').collect::<Vec<_>>().as_slice() {
            push_options.insert(key.to_string(), value.to_string());
        } else {
            push_options.insert(s, "".to_string());
        }
    }

    std::fs::write(p, serde_json::to_string(&push_options)?)?;

    Ok(0)
}

fn update_hook(refname: &str, old: &str, new: &str) -> josh::JoshResult<i32> {
    let mut repo_update: josh_proxy::RepoUpdate =
        serde_json::from_str(&std::env::var("JOSH_REPO_UPDATE")?)?;

    repo_update
        .refs
        .insert(refname.to_owned(), (old.to_owned(), new.to_owned()));

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let resp = client
        .post(&format!(
            "http://localhost:{}/repo_update",
            repo_update.port
        ))
        .json(&repo_update)
        .send();

    match resp {
        Ok(r) => {
            let success = r.status().is_success();
            if let Ok(body) = r.text() {
                println!("response from upstream:\n{}\n\n", body);
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
    Ok(1)
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
    if let [a0, a1, a2, a3, ..] = &std::env::args().collect::<Vec<_>>().as_slice() {
        if a0.ends_with("/update") {
            std::process::exit(update_hook(a1, a2, a3).unwrap_or(1));
        }
    }

    if let [a0, ..] = &std::env::args().collect::<Vec<_>>().as_slice() {
        if a0.ends_with("/pre-receive") {
            println!("josh-proxy");
            std::process::exit(pre_receive_hook().unwrap_or(1));
        }
    }

    let fmt_layer = tracing_subscriber::fmt::layer().compact().with_ansi(false);

    let filter = match std::env::var("RUST_LOG") {
        Ok(_) => tracing_subscriber::EnvFilter::from_default_env(),
        _ => tracing_subscriber::EnvFilter::new("josh=trace,josh_proxy=trace"),
    };

    if let Ok(endpoint) = std::env::var("JOSH_JAEGER_ENDPOINT") {
        let tracer = opentelemetry_jaeger::new_pipeline()
            .with_service_name(
                std::env::var("JOSH_SERVICE_NAME").unwrap_or("josh-proxy".to_owned()),
            )
            .with_agent_endpoint(endpoint)
            .install_simple()
            .expect("can't install opentelemetry pipeline");

        let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = filter
            .and_then(fmt_layer)
            .and_then(telemetry_layer)
            .with_subscriber(tracing_subscriber::Registry::default());
        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");
    } else {
        let subscriber = filter
            .and_then(fmt_layer)
            .with_subscriber(tracing_subscriber::Registry::default());
        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");
    };

    std::process::exit(run_proxy().unwrap_or(1));
}

#[test]
fn verify_app() {
    make_app().debug_assert();
}
