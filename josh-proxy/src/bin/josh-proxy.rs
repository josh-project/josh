#![deny(warnings)]
#[macro_use]
extern crate lazy_static;

use josh_proxy::{FetchError, MetaConfig, RemoteAuth, RepoConfig, RepoUpdate};
use opentelemetry::global;
use opentelemetry::sdk::propagation::TraceContextPropagator;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::Layer;

use futures::future;
use futures::FutureExt;
use hyper::body::HttpBody;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server, StatusCode};
use hyper_reverse_proxy;
use indoc::formatdoc;
use josh::{josh_error, JoshError};
use josh_rpc::calls::RequestedCommand;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::process::Command;
use tracing::{trace, Span};
use tracing_futures::Instrument;

fn version_str() -> String {
    format!("Version: {}\n", josh::VERSION,)
}

lazy_static! {
    static ref ARGS: josh_proxy::cli::Args = josh_proxy::cli::parse_args_or_exit(1);
}

josh::regex_parsed!(
    FilteredRepoUrl,
    r"(?P<api>/~/\w+)?(?P<upstream_repo>/[^:!]*[.]git)(?P<headref>[\^@][^:!]*)?((?P<filter_spec>[:!].*)[.]git)?(?P<pathinfo>/.*)?(?P<rest>.*)",
    [api, upstream_repo, filter_spec, pathinfo, headref, rest]
);

type FetchTimers = HashMap<String, std::time::Instant>;
type Polls =
    Arc<std::sync::Mutex<std::collections::HashSet<(String, josh_proxy::auth::Handle, String)>>>;

type HeadsMap = Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>;

#[derive(Serialize, Clone, Debug)]
enum JoshProxyUpstream {
    Http(String),
    Ssh(String),
    Both { http: String, ssh: String },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum UpstreamProtocol {
    Http,
    Ssh,
}

impl JoshProxyUpstream {
    fn get(&self, protocol: UpstreamProtocol) -> Option<String> {
        match (self, protocol) {
            (JoshProxyUpstream::Http(http), UpstreamProtocol::Http)
            | (JoshProxyUpstream::Both { http, .. }, UpstreamProtocol::Http) => Some(http.clone()),
            (JoshProxyUpstream::Ssh(ssh), UpstreamProtocol::Ssh)
            | (JoshProxyUpstream::Both { http: _, ssh }, UpstreamProtocol::Ssh) => {
                Some(ssh.clone())
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
struct JoshProxyService {
    port: String,
    repo_path: std::path::PathBuf,
    upstream: JoshProxyUpstream,
    fetch_timers: Arc<RwLock<FetchTimers>>,
    heads_map: HeadsMap,
    fetch_permits: Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    filter_permits: Arc<tokio::sync::Semaphore>,
    poll: Polls,
}

impl std::fmt::Debug for JoshProxyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("upstream", &self.upstream)
            .finish()
    }
}

#[tracing::instrument]
async fn fetch_upstream(
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    remote_auth: &RemoteAuth,
    remote_url: String,
    headref: &str,
    force: bool,
) -> Result<(), FetchError> {
    let key = remote_url.clone();

    let refs_to_fetch =
        if !headref.is_empty() && headref != "HEAD" && !headref.starts_with("refs/heads/") {
            vec![
                "HEAD*",
                "refs/josh/*",
                "refs/heads/*",
                "refs/tags/*",
                headref,
            ]
        } else {
            vec!["HEAD*", "refs/josh/*", "refs/heads/*", "refs/tags/*"]
        };

    let refs_to_fetch: Vec<_> = refs_to_fetch.iter().map(|x| x.to_string()).collect();

    let fetch_cached_ok = {
        if let Some(last) = service.fetch_timers.read()?.get(&key) {
            let since = std::time::Instant::now().duration_since(*last);
            let max = std::time::Duration::from_secs(ARGS.cache_duration);

            tracing::trace!("last: {:?}, since: {:?}, max: {:?}", last, since, max);
            since < max
        } else {
            false
        }
    };

    let fetch_cached_ok = fetch_cached_ok && !force;

    tracing::trace!("fetch_cached_ok {:?}", fetch_cached_ok);

    if fetch_cached_ok && headref.is_empty() {
        return Ok(());
    }

    if fetch_cached_ok && !headref.is_empty() {
        let transaction = josh::cache::Transaction::open(
            &service.repo_path.join("mirror"),
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&upstream_repo),
            )),
        )
        .map_err(FetchError::from_josh_error)?;
        let id = transaction
            .repo()
            .refname_to_id(&transaction.refname(headref));
        tracing::trace!("refname_to_id: {:?}", id);
        if id.is_ok() {
            return Ok(());
        }
    }

    let fetch_timers = service.fetch_timers.clone();
    let heads_map = service.heads_map.clone();
    let br_path = service.repo_path.join("mirror");

    let span = tracing::span!(tracing::Level::TRACE, "fetch worker");
    let us = upstream_repo.clone();
    let ru = remote_url.clone();
    let semaphore = service
        .fetch_permits
        .lock()?
        .entry(us.clone())
        .or_insert(Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();
    let permit = semaphore.acquire().await;
    let task_remote_auth = remote_auth.clone();
    let fetch_result = tokio::task::spawn_blocking(move || {
        let _span_guard = span.enter();
        josh_proxy::fetch_refs_from_url(&br_path, &us, &ru, &refs_to_fetch, &task_remote_auth)
    })
    .await?;

    let us = upstream_repo.clone();
    let s = tracing::span!(tracing::Level::TRACE, "get_head worker");
    let br_path = service.repo_path.join("mirror");
    let ru = remote_url.clone();
    let task_remote_auth = remote_auth.clone();
    let hres = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        josh_proxy::get_head(&br_path, &ru, &task_remote_auth)
    })
    .await?;

    if let Ok(hres) = hres {
        heads_map.write()?.insert(us, hres);
    }

    std::mem::drop(permit);

    match (fetch_result, remote_auth) {
        (Ok(_), RemoteAuth::Http { auth }) => {
            fetch_timers.write()?.insert(key, std::time::Instant::now());

            let (auth_user, _) = auth.parse().map_err(FetchError::from_josh_error)?;

            if matches!(&ARGS.poll_user, Some(user) if auth_user == user.as_str()) {
                service
                    .poll
                    .lock()?
                    .insert((upstream_repo, auth.clone(), remote_url));
            }

            Ok(())
        }
        (Ok(_), _) => Ok(()),
        (Err(e), _) => Err(e),
    }
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
        return match service.upstream.get(UpstreamProtocol::Http) {
            None => Ok(Some(make_response(
                hyper::Body::from("HTTP remote is not configured"),
                hyper::StatusCode::SERVICE_UNAVAILABLE,
            ))),
            Some(remote) => Ok(Some(make_response(
                hyper::Body::from(remote),
                hyper::StatusCode::OK,
            ))),
        };
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
            let transaction_mirror =
                josh::cache::Transaction::open(&service.repo_path.join("mirror"), None)?;
            josh::housekeeping::discover_filter_candidates(&transaction_mirror)?;
            if refresh {
                let transaction_overlay =
                    josh::cache::Transaction::open(&service.repo_path.join("overlay"), None)?;
                josh::housekeeping::refresh_known_filters(
                    &transaction_mirror,
                    &transaction_overlay,
                )?;
            }
            Ok(toml::to_string_pretty(
                &josh::housekeeping::get_known_filters()?,
            )?)
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
        let repo_update: RepoUpdate = serde_json::from_str(buffer)?;
        let context_propagator = repo_update.context_propagator.clone();
        let parent_context =
            global::get_text_map_propagator(|propagator| propagator.extract(&context_propagator));
        s.set_parent(parent_context);

        josh_proxy::process_repo_update(repo_update)
    })
    .instrument(Span::current())
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
    meta: josh_proxy::MetaConfig,
    temp_ns: Arc<josh_proxy::TmpGitNamespace>,
    filter: josh::filter::Filter,
    headref: String,
) -> josh::JoshResult<()> {
    let permit = service.filter_permits.acquire().await;
    let heads_map = service.heads_map.clone();

    let s = tracing::span!(tracing::Level::TRACE, "do_filter worker");
    let r = tokio::task::spawn_blocking(move || {
        let _e = s.enter();
        tracing::trace!("in do_filter worker");
        let filter_spec = josh::filter::spec(filter);
        josh::housekeeping::remember_filter(&meta.config.repo, &filter_spec);

        let transaction = josh::cache::Transaction::open(
            &repo_path.join("mirror"),
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&meta.config.repo),
            )),
        )?;
        let mut refslist = josh::housekeeping::list_refs(transaction.repo(), &meta.config.repo)?;

        let mut headref = headref;

        if headref.starts_with("refs/") || headref == "HEAD" {
            let name = format!(
                "refs/josh/upstream/{}/{}",
                &josh::to_ns(&meta.config.repo),
                headref.clone()
            );
            if let Ok(r) = transaction.repo().revparse_single(&name) {
                refslist.push((headref.clone(), r.id()));
            }
        } else {
            // @sha case
            refslist.push((headref.clone(), git2::Oid::from_str(&headref)?));
            headref = format!("refs/heads/_{}", headref);
        }

        if headref == "HEAD" {
            headref = heads_map
                .read()?
                .get(&meta.config.repo)
                .unwrap_or(&"invalid".to_string())
                .clone();
        }

        let t2 = josh::cache::Transaction::open(&repo_path.join("overlay"), None)?;
        t2.repo()
            .odb()?
            .add_disk_alternate(&repo_path.join("mirror").join("objects").to_str().unwrap())?;
        let updated_refs = josh::filter_refs(&t2, filter, &refslist, josh::filter::empty())?;
        let mut updated_refs = josh_proxy::refs_locking(updated_refs, &meta);
        josh::housekeeping::namespace_refs(&mut updated_refs, &temp_ns.name());
        josh::update_refs(&t2, &mut updated_refs, &temp_ns.reference(&headref));
        t2.repo()
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

async fn handle_ui_request(
    req: Request<hyper::Body>,
    resource_path: &str,
) -> josh::JoshResult<Response<hyper::Body>> {
    // Proxy: can be used for UI development or to serve a different UI
    if let Some(proxy) = &ARGS.static_resource_proxy_target {
        let client_ip = IpAddr::from_str("127.0.0.1").unwrap();
        return match hyper_reverse_proxy::call(client_ip, proxy, req).await {
            Ok(response) => Ok(response),
            Err(error) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(hyper::Body::from(format!("Proxy error: {:?}", error)))
                .unwrap()),
        };
    }

    // Serve prebuilt UI from static resources dir
    let is_app_route = resource_path == "/"
        || resource_path == "/select"
        || resource_path == "/browse"
        || resource_path == "/view"
        || resource_path == "/diff"
        || resource_path == "/change"
        || resource_path == "/history";

    let resolve_path = if is_app_route {
        "index.html"
    } else {
        resource_path
    };

    let result = hyper_staticfile::resolve_path("/josh/static", resolve_path).await?;
    let response = hyper_staticfile::ResponseBuilder::new()
        .request(&req)
        .build(result)?;

    return Ok(response);
}

async fn query_meta_repo(
    serv: Arc<JoshProxyService>,
    meta_repo: &str,
    upstream_protocol: UpstreamProtocol,
    upstream_repo: &str,
    remote_auth: &RemoteAuth,
) -> josh::JoshResult<josh_proxy::MetaConfig> {
    let upstream = serv
        .upstream
        .get(upstream_protocol)
        .ok_or(josh_error("no remote specified for the requested protocol"))?;

    let remote_url = [upstream.as_str(), meta_repo].join("");
    match fetch_upstream(
        serv.clone(),
        meta_repo.to_owned(),
        &remote_auth,
        remote_url.to_owned(),
        &"HEAD",
        false,
    )
    .in_current_span()
    .await
    {
        Ok(_) => {}
        Err(FetchError::AuthRequired) => return Err(josh_error("meta fetch: auth failed")),
        Err(FetchError::Other(e)) => return Err(josh_error(&format!("meta fetch failed: {}", e))),
    }

    let transaction = josh::cache::Transaction::open(
        &serv.repo_path.join("mirror"),
        Some(&format!("refs/josh/upstream/{}/", &josh::to_ns(&meta_repo),)),
    )?;

    let meta_tree = transaction
        .repo()
        .find_reference(&transaction.refname("HEAD"))?
        .peel_to_tree()?;

    let meta_blob = josh::filter::tree::get_blob(
        transaction.repo(),
        &meta_tree,
        &std::path::Path::new(&upstream_repo.trim_start_matches("/")).join("config.yml"),
    );

    if meta_blob == "" {
        return Err(josh::josh_error(&"meta repo entry not found"));
    }

    let mut meta: josh_proxy::MetaConfig = Default::default();

    meta.config = serde_yaml::from_str(&meta_blob)?;

    if meta.config.lock_refs {
        let meta_blob = josh::filter::tree::get_blob(
            transaction.repo(),
            &meta_tree,
            &std::path::Path::new(&upstream_repo.trim_start_matches("/")).join("lock.yml"),
        );

        if meta_blob == "" {
            return Err(josh::josh_error(&"locked refs not found"));
        }
        meta.refs_lock = serde_yaml::from_str(&meta_blob)?;
    }

    return Ok(meta);
}

async fn make_meta_config(
    serv: Arc<JoshProxyService>,
    remote_auth: &RemoteAuth,
    upstream_protocol: UpstreamProtocol,
    parsed_url: &FilteredRepoUrl,
) -> josh::JoshResult<MetaConfig> {
    let meta_repo = std::env::var("JOSH_META_REPO");
    let auth_token = std::env::var("JOSH_META_AUTH_TOKEN");

    match meta_repo {
        Err(_) => Ok(MetaConfig {
            config: RepoConfig {
                repo: parsed_url.upstream_repo.clone(),
                ..Default::default()
            },
            ..Default::default()
        }),
        Ok(meta_repo) => {
            let auth = match remote_auth {
                RemoteAuth::Ssh { auth_socket } => RemoteAuth::Ssh {
                    auth_socket: auth_socket.clone(),
                },
                RemoteAuth::Http { auth } => {
                    let auth = if let Ok(token) = auth_token {
                        josh_proxy::auth::add_auth(&token)?
                    } else {
                        auth.clone()
                    };

                    RemoteAuth::Http { auth }
                }
            };

            query_meta_repo(
                serv.clone(),
                &meta_repo,
                upstream_protocol,
                &parsed_url.upstream_repo,
                &auth,
            )
            .await
        }
    }
}

async fn serve_namespace(
    params: &josh_rpc::calls::ServeNamespace,
    repo_path: std::path::PathBuf,
    namespace: &str,
) -> josh::JoshResult<()> {
    const SERVE_TIMEOUT: u64 = 60;

    tracing::trace!(
        "serve_namespace: command: {:?}, query: {}, namespace: {}",
        params.command,
        params.query,
        namespace
    );

    enum ServeError {
        FifoError(std::io::Error),
        SubprocessError(std::io::Error),
        SubprocessTimeout(tokio::time::error::Elapsed),
        SubprocessExited(i32),
    }

    if params.command == RequestedCommand::GitReceivePack {
        return Err(josh_error("Push over SSH is not supported"));
    }

    let command = match params.command {
        RequestedCommand::GitUploadPack => "git-upload-pack",
        RequestedCommand::GitUploadArchive => "git-upload-archive",
        RequestedCommand::GitReceivePack => "git-receive-pack",
    };

    let mut process = tokio::process::Command::new(command)
        .arg(repo_path.join("overlay"))
        .current_dir(repo_path.join("overlay"))
        .env("GIT_DIR", &repo_path)
        .env("GIT_NAMESPACE", namespace)
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            repo_path.join("mirror").join("objects"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = process.stdout.take().ok_or(josh_error("no stdout"))?;
    let stdin = process.stdin.take().ok_or(josh_error("no stdin"))?;

    let stdin_cancel_token = tokio_util::sync::CancellationToken::new();
    let stdin_cancel_token_stdout = stdin_cancel_token.clone();

    let read_stdout = async {
        // If stdout stream was closed, cancel stdin copy future
        let _guard_stdin = stdin_cancel_token_stdout.drop_guard();

        let copy_future = async {
            // Move stdout here because it should be closed after copy,
            // and to be closed it needs to be dropped
            let mut stdout = stdout;

            // Dropping the handle at the end of this block will generate EOF at the other end
            let mut stdout_stream = UnixStream::connect(&params.stdout_sock).await?;

            tokio::io::copy(&mut stdout, &mut stdout_stream).await?;
            stdout_stream.flush().await
        };

        copy_future.await.map_err(|e| ServeError::FifoError(e))
    };

    let write_stdin = async {
        // When stdout copying was finished (subprocess closed their
        // stdout due to termination), we need to cancel this future.
        // Future cancelling is implemented via token.
        let copy_future = async {
            // See comment about stdout above
            let mut stdin = stdin;
            let mut stdin_stream = UnixStream::connect(&params.stdin_sock).await?;

            tokio::io::copy(&mut stdin_stream, &mut stdin).await?;

            // Flushing is necessary to ensure file handle is closed when
            // it goes out of scope / dropped
            stdin.flush().await
        };

        tokio::select! {
            copy_result = copy_future => {
                copy_result
                    .map(|_| ())
                    .map_err(|e| ServeError::FifoError(e))
            }
            _ = stdin_cancel_token.cancelled() => {
                Ok(())
            }
        }
    };

    let maybe_process_completion = async {
        let max_duration = tokio::time::Duration::from_secs(SERVE_TIMEOUT);
        match tokio::time::timeout(max_duration, process.wait()).await {
            Ok(status) => match status {
                Ok(status) => match status.code() {
                    Some(code) if code == 0 => Ok(()),
                    Some(code) => Err(ServeError::SubprocessExited(code)),
                    None => {
                        let io_error = std::io::Error::from(std::io::ErrorKind::Other);
                        Err(ServeError::SubprocessError(io_error))
                    }
                },
                Err(io_error) => Err(ServeError::SubprocessError(io_error)),
            },
            Err(elapsed) => Err(ServeError::SubprocessTimeout(elapsed)),
        }
    };

    let subprocess_result = tokio::try_join!(read_stdout, write_stdin, maybe_process_completion);

    match subprocess_result {
        Ok(_) => Ok(()),
        Err(e) => match e {
            ServeError::SubprocessExited(code) => Err(josh_error(&format!(
                "git subprocess exited with code {}",
                code
            ))),
            ServeError::SubprocessError(io_error) => Err(josh_error(&format!(
                "could not start git subprocess: {}",
                io_error
            ))),
            ServeError::SubprocessTimeout(elapsed) => {
                let _ = process.kill().await;
                Err(josh_error(&format!(
                    "git subprocess timed out after {}",
                    elapsed
                )))
            }
            ServeError::FifoError(io_error) => {
                let _ = process.kill().await;
                Err(josh_error(&format!(
                    "git subprocess communication error: {}",
                    io_error
                )))
            }
        },
    }
}

fn is_repo_blocked(meta: &MetaConfig) -> bool {
    let block = std::env::var("JOSH_REPO_BLOCK").unwrap_or("".to_owned());
    let block = block.split(";").collect::<Vec<_>>();

    for b in block {
        if b == meta.config.repo {
            return true;
        }
    }

    false
}

fn headref_or_default(headref: &str) -> String {
    let result = headref
        .trim_start_matches(|char| char == '@' || char == '^')
        .to_owned();

    if result.is_empty() {
        "HEAD".to_string()
    } else {
        result
    }
}

async fn handle_serve_namespace_request(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
) -> josh::JoshResult<Response<hyper::Body>> {
    let error_response = |status: StatusCode| Ok(make_response(hyper::Body::empty(), status));

    if req.method() != hyper::Method::POST {
        return error_response(StatusCode::METHOD_NOT_ALLOWED);
    }

    match req.headers().get(hyper::header::CONTENT_TYPE) {
        Some(value) if value == "application/json" => (),
        _ => return error_response(StatusCode::BAD_REQUEST),
    }

    let body = match req.into_body().data().await {
        None => return error_response(StatusCode::BAD_REQUEST),
        Some(result) => match result {
            Ok(bytes) => bytes,
            Err(_) => return error_response(StatusCode::IM_A_TEAPOT),
        },
    };

    let params = match serde_json::from_slice::<josh_rpc::calls::ServeNamespace>(&body) {
        Err(error) => {
            return Ok(make_response(
                hyper::Body::from(error.to_string()),
                StatusCode::BAD_REQUEST,
            ))
        }
        Ok(parsed) => parsed,
    };

    let parsed_url = if let Some(mut parsed_url) = FilteredRepoUrl::from_str(&params.query) {
        if parsed_url.filter_spec.is_empty() {
            parsed_url.filter_spec = ":/".to_string();
        }

        parsed_url
    } else {
        return Ok(make_response(
            hyper::Body::from("Unable to parse query"),
            StatusCode::BAD_REQUEST,
        ));
    };

    let remote_auth = RemoteAuth::Ssh {
        auth_socket: params.ssh_socket.clone(),
    };

    let meta_config = match make_meta_config(
        serv.clone(),
        &remote_auth,
        UpstreamProtocol::Ssh,
        &parsed_url,
    )
    .await
    {
        Ok(meta) => meta,
        Err(e) => {
            return Ok(make_response(
                hyper::Body::from(format!("Error fetching meta repo: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    if is_repo_blocked(&meta_config) {
        return Ok(make_response(
            hyper::Body::from("Access to this repo is blocked via JOSH_REPO_BLOCK"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ));
    }

    let upstream = match serv.upstream.get(UpstreamProtocol::Ssh) {
        Some(upstream) => upstream,
        None => {
            return Ok(make_response(
                hyper::Body::from("SSH remote is not configured"),
                hyper::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    };

    let remote_url = upstream + meta_config.config.repo.as_str();
    let headref = headref_or_default(&parsed_url.headref);

    match fetch_upstream(
        serv.clone(),
        meta_config.config.repo.to_owned(),
        &remote_auth,
        remote_url.to_owned(),
        &headref,
        true,
    )
    .await
    {
        Ok(_) => {}
        Err(FetchError::AuthRequired) => {
            return Ok(make_response(
                hyper::Body::from("Access to upstream repo denied"),
                StatusCode::FORBIDDEN,
            ))
        }
        Err(FetchError::Other(e)) => {
            return Ok(make_response(
                hyper::Body::from(e.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }

    trace!(
        "handle_serve_namespace_request: filter_spec: {}",
        parsed_url.filter_spec
    );

    let query_filter = match josh::filter::parse(&parsed_url.filter_spec) {
        Ok(filter) => filter,
        Err(e) => {
            return Ok(make_response(
                hyper::Body::from(format!("Failed to parse filter: {}", e.to_string())),
                StatusCode::BAD_REQUEST,
            ))
        }
    };

    let filter = josh::filter::chain(
        query_filter,
        match &ARGS.filter_prefix {
            Some(filter_prefix) => {
                let filter_prefix = match josh::filter::parse(&filter_prefix) {
                    Ok(filter) => filter,
                    Err(e) => {
                        return Ok(make_response(
                            hyper::Body::from(format!(
                                "Failed to parse prefix filter passed as command line argument: {}",
                                e.to_string()
                            )),
                            StatusCode::SERVICE_UNAVAILABLE,
                        ))
                    }
                };

                josh::filter::chain(meta_config.config.filter, filter_prefix)
            }
            None => meta_config.config.filter,
        },
    );

    let temp_ns = match prepare_namespace(serv.clone(), &meta_config, filter, &headref).await {
        Ok(ns) => ns,
        Err(e) => {
            return Ok(make_response(
                hyper::Body::from(e.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    };

    let serve_result = serve_namespace(&params, serv.repo_path.clone(), temp_ns.name()).await;
    std::mem::drop(temp_ns);

    match serve_result {
        Ok(_) => Ok(make_response(hyper::Body::empty(), StatusCode::NO_CONTENT)),
        Err(e) => Ok(make_response(
            hyper::Body::from(e.to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

#[tracing::instrument]
async fn call_service(
    serv: Arc<JoshProxyService>,
    req_auth: (josh_proxy::auth::Handle, Request<hyper::Body>),
) -> josh::JoshResult<Response<hyper::Body>> {
    let (auth, req) = req_auth;

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if let Some(resource_path) = path.strip_prefix("/~/ui") {
        return handle_ui_request(req, resource_path).await;
    }

    if let Some(response) = static_paths(&serv, &path).await? {
        return Ok(response);
    }

    // When exposed to internet, should be blocked
    if path == "/repo_update" {
        return repo_update_fn(serv, req).await;
    }

    if path == "/serve_namespace" {
        return handle_serve_namespace_request(serv.clone(), req).await;
    }

    // Need to have some way of passing the filter (via remote path like what github does?)
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

            if pu.filter_spec.is_empty() {
                pu.filter_spec = ":/".to_string();
            }
            pu
        } else {
            let redirect_path = if path == "/" {
                "/~/ui/".to_string()
            } else {
                format!(
                    "/~/ui/browse?repo={}.git&path=&filter=%3A%2F&rev=HEAD",
                    path
                )
            };

            return Ok(Response::builder()
                .status(hyper::StatusCode::FOUND)
                .header("Location", redirect_path)
                .body(hyper::Body::empty())?);
        }
    };

    let remote_auth = RemoteAuth::Http { auth: auth.clone() };
    let meta = make_meta_config(
        serv.clone(),
        &remote_auth,
        UpstreamProtocol::Http,
        &parsed_url,
    )
    .await?;

    let mut filter = josh::filter::chain(
        meta.config.filter,
        josh::filter::parse(&parsed_url.filter_spec)?,
    );

    let upstream = match serv.upstream.get(UpstreamProtocol::Http) {
        Some(upstream) => upstream,
        None => {
            return Ok(make_response(
                hyper::Body::from("HTTP remote is not configured"),
                hyper::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    };

    let remote_url = upstream + meta.config.repo.as_str();

    if let Some(filter_prefix) = &ARGS.filter_prefix {
        filter = josh::filter::chain(josh::filter::parse(filter_prefix)?, filter);
    }

    if parsed_url.pathinfo.starts_with("/info/lfs") {
        return Ok(Response::builder()
            .status(hyper::StatusCode::TEMPORARY_REDIRECT)
            .header("Location", format!("{}{}", remote_url, parsed_url.pathinfo))
            .body(hyper::Body::empty())?);
    }

    if is_repo_blocked(&meta) {
        return Ok(make_response(
            hyper::Body::from(formatdoc!(
                r#"
                    Access to this repo is blocked via JOSH_REPO_BLOCK
                    "#
            )),
            hyper::StatusCode::UNPROCESSABLE_ENTITY,
        ));
    }

    let http_auth_required = ARGS.require_auth && parsed_url.pathinfo == "/git-receive-pack";

    if !josh_proxy::auth::check_auth(&remote_url, &auth, http_auth_required)
        .in_current_span()
        .await?
    {
        tracing::trace!("require-auth");
        let builder = Response::builder()
            .header(
                hyper::header::WWW_AUTHENTICATE,
                "Basic realm=User Visible Realm",
            )
            .status(hyper::StatusCode::UNAUTHORIZED);
        return Ok(builder.body(hyper::Body::empty())?);
    }

    if parsed_url.api == "/~/graphql" {
        return serve_graphql(serv, req, meta.config.repo.to_owned(), remote_url, auth).await;
    }

    if parsed_url.api == "/~/graphiql" {
        let addr = format!("/~/graphql{}", meta.config.repo);
        return Ok(tokio::task::spawn_blocking(move || {
            josh_proxy::juniper_hyper::graphiql(&addr, None)
        })
        .in_current_span()
        .await??);
    }

    let headref = headref_or_default(&parsed_url.headref);
    match fetch_upstream(
        serv.clone(),
        meta.config.repo.to_owned(),
        &remote_auth,
        remote_url.to_owned(),
        &headref,
        false,
    )
    .in_current_span()
    .await
    {
        Ok(_) => {}
        Err(FetchError::AuthRequired) => {
            let builder = Response::builder()
                .header(
                    hyper::header::WWW_AUTHENTICATE,
                    "Basic realm=User Visible Realm",
                )
                .status(hyper::StatusCode::UNAUTHORIZED);
            return Ok(builder.body(hyper::Body::empty())?);
        }
        Err(FetchError::Other(e)) => {
            let builder = Response::builder().status(hyper::StatusCode::INTERNAL_SERVER_ERROR);
            return Ok(builder.body(hyper::Body::from(e.0))?);
        }
    }

    if let (Some(q), true) = (
        req.uri().query().map(|x| x.to_string()),
        parsed_url.pathinfo.is_empty(),
    ) {
        return serve_query(serv, q, meta.config.repo, filter, headref).await;
    }

    let temp_ns = prepare_namespace(serv.clone(), &meta, filter, &headref)
        .in_current_span()
        .await?;

    let repo_path = serv
        .repo_path
        .join("overlay")
        .to_str()
        .ok_or(josh::josh_error("repo_path.to_str"))?
        .to_string();

    let mirror_repo_path = serv
        .repo_path
        .join("mirror")
        .to_str()
        .ok_or(josh::josh_error("repo_path.to_str"))?
        .to_string();

    let span = tracing::span!(tracing::Level::TRACE, "hyper_cgi");
    let _enter = span.enter();
    let mut context_propagator = HashMap::<String, String>::default();
    let context = span.context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut context_propagator);
    });
    tracing::warn!("debug propagator: {:?}", context_propagator);

    let repo_update = josh_proxy::RepoUpdate {
        refs: HashMap::new(),
        remote_url: remote_url.clone(),
        remote_auth,
        port: serv.port.clone(),
        filter_spec: josh::filter::spec(filter),
        base_ns: josh::to_ns(&meta.config.repo),
        git_ns: temp_ns.name().to_string(),
        git_dir: repo_path.clone(),
        mirror_git_dir: mirror_repo_path.clone(),
        context_propagator: context_propagator,
    };

    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&serv.repo_path.join("overlay"));
    cmd.env("GIT_DIR", &repo_path);
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.env(
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        serv.repo_path
            .join("mirror")
            .join("objects")
            .to_str()
            .ok_or(josh::josh_error("repo_path.to_str"))?,
    );
    cmd.env("GIT_NAMESPACE", temp_ns.name().clone());
    cmd.env("GIT_PROJECT_ROOT", repo_path);
    cmd.env("JOSH_REPO_UPDATE", serde_json::to_string(&repo_update)?);
    cmd.env("PATH_INFO", parsed_url.pathinfo.clone());

    let git_span = tracing::span!(tracing::Level::TRACE, "git http backend");
    let cgires = hyper_cgi::do_cgi(req, cmd).instrument(git_span).await;

    tracing::debug!(
        "Git stderr: {}",
        String::from_utf8(cgires.1).unwrap_or("".to_string())
    );

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    std::mem::drop(temp_ns);

    Ok(cgires.0)
}

async fn serve_query(
    serv: Arc<JoshProxyService>,
    q: String,
    upstream_repo: String,
    filter: josh::filter::Filter,
    headref: String,
) -> josh::JoshResult<Response<hyper::Body>> {
    let s = tracing::span!(tracing::Level::TRACE, "render worker");
    let res = tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
        let _e = s.enter();

        let transaction_mirror = josh::cache::Transaction::open(
            &serv.repo_path.join("mirror"),
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&upstream_repo),
            )),
        )?;

        let transaction = josh::cache::Transaction::open(&serv.repo_path.join("overlay"), None)?;
        transaction.repo().odb()?.add_disk_alternate(
            &serv
                .repo_path
                .join("mirror")
                .join("objects")
                .to_str()
                .unwrap(),
        )?;

        let commit_id = transaction_mirror
            .repo()
            .refname_to_id(&transaction_mirror.refname(&headref))?;
        let commit_id =
            josh::filter_commit(&transaction, filter, commit_id, josh::filter::empty())?;

        josh::query::render(&transaction, "", commit_id, &q)
    })
    .in_current_span()
    .await?;

    return Ok(match res {
        Ok(Some(res)) => Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(res))?,

        Ok(None) => Response::builder()
            .status(hyper::StatusCode::NOT_FOUND)
            .body(hyper::Body::from("File not found".to_string()))?,

        Err(res) => Response::builder()
            .status(hyper::StatusCode::UNPROCESSABLE_ENTITY)
            .body(hyper::Body::from(res.to_string()))?,
    });
}

#[tracing::instrument]
async fn prepare_namespace(
    serv: Arc<JoshProxyService>,
    meta: &josh_proxy::MetaConfig,
    filter: josh::filter::Filter,
    headref: &str,
) -> josh::JoshResult<std::sync::Arc<josh_proxy::TmpGitNamespace>> {
    let temp_ns = Arc::new(josh_proxy::TmpGitNamespace::new(
        &serv.repo_path.join("overlay"),
        tracing::Span::current(),
    ));

    let serv = serv.clone();

    do_filter(
        serv.repo_path.clone(),
        serv.clone(),
        meta.clone(),
        temp_ns.to_owned(),
        filter,
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
    let addr = format!("0.0.0.0:{}", ARGS.port).parse()?;
    let upstream = match (&ARGS.remote.http, &ARGS.remote.ssh) {
        (Some(http), None) => JoshProxyUpstream::Http(http.clone()),
        (None, Some(ssh)) => JoshProxyUpstream::Ssh(ssh.clone()),
        (Some(http), Some(ssh)) => JoshProxyUpstream::Both {
            http: http.clone(),
            ssh: ssh.clone(),
        },
        (None, None) => return Err(josh_error("missing remote host url")),
    };

    let local = std::path::PathBuf::from(&ARGS.local);
    let local = if local.is_absolute() {
        local
    } else {
        std::env::current_dir()?.join(local)
    };

    josh_proxy::create_repo(&local)?;
    josh::cache::load(&local)?;

    let proxy_service = Arc::new(JoshProxyService {
        port: ARGS.port.to_string(),
        repo_path: local.to_owned(),
        upstream,
        fetch_timers: Arc::new(RwLock::new(FetchTimers::new())),
        heads_map: Arc::new(RwLock::new(std::collections::HashMap::new())),
        poll: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        fetch_permits: Default::default(),
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

    if ARGS.no_background {
        tokio::select!(
            r = server_future => println!("http server exited: {:?}", r),
        );
    } else {
        tokio::select!(
            r = run_housekeeping(local) => println!("run_housekeeping exited: {:?}", r),
            r = run_polling(ps.clone()) => println!("run_polling exited: {:?}", r),
            r = server_future => println!("http server exited: {:?}", r),
        );
    }
    Ok(0)
}

async fn run_polling(serv: Arc<JoshProxyService>) -> josh::JoshResult<()> {
    loop {
        let polls = serv.poll.lock()?.clone();

        for (upstream_repo, auth, url) in polls {
            let remote_auth = RemoteAuth::Http { auth };
            let fetch_result = fetch_upstream(
                serv.clone(),
                upstream_repo.clone(),
                &remote_auth,
                url.clone(),
                "",
                true,
            )
            .in_current_span()
            .await;

            match fetch_result {
                Ok(()) => {}
                Err(FetchError::Other(e)) => return Err(e),
                Err(FetchError::AuthRequired) => {
                    return Err(josh_error("auth: access denied while polling"))
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

async fn run_housekeeping(local: std::path::PathBuf) -> josh::JoshResult<()> {
    let mut i: usize = 0;
    loop {
        let local = local.clone();
        tokio::task::spawn_blocking(move || {
            josh::housekeeping::run(&local, (i % 60 == 0) && ARGS.gc)
        })
        .await??;
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        i += 1;
    }
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

async fn serve_graphql(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
    upstream_repo: String,
    remote_url: String,
    auth: josh_proxy::auth::Handle,
) -> josh::JoshResult<Response<hyper::Body>> {
    let parsed = match josh_proxy::juniper_hyper::parse_req(req).await {
        Ok(r) => r,
        Err(resp) => return Ok(resp),
    };

    let transaction_mirror = josh::cache::Transaction::open(
        &serv.repo_path.join("mirror"),
        Some(&format!(
            "refs/josh/upstream/{}/",
            &josh::to_ns(&upstream_repo),
        )),
    )?;
    let transaction = josh::cache::Transaction::open(&serv.repo_path.join("overlay"), None)?;
    transaction.repo().odb()?.add_disk_alternate(
        &serv
            .repo_path
            .join("mirror")
            .join("objects")
            .to_str()
            .unwrap(),
    )?;
    let context = std::sync::Arc::new(josh::graphql::context(transaction, transaction_mirror));
    let root_node = std::sync::Arc::new(josh::graphql::repo_schema(
        upstream_repo
            .strip_suffix(".git")
            .unwrap_or(&upstream_repo)
            .to_string(),
        false,
    ));

    let remote_auth = RemoteAuth::Http { auth };
    let res = {
        // First attempt to serve GraphQL query. If we can serve it
        // that means all requested revisions were specified by SHA and we could find
        // all of them locally, so no need to fetch.
        let res = parsed.execute(&root_node, &context).await;

        // The "allow_refs" flag will be set by the query handler if we need to do a fetch
        // to complete the query.
        if !*context.allow_refs.lock().unwrap() {
            res
        } else {
            match fetch_upstream(
                serv.clone(),
                upstream_repo.to_owned(),
                &remote_auth,
                remote_url.to_owned(),
                &"HEAD",
                false,
            )
            .in_current_span()
            .await
            {
                Ok(_) => {}
                Err(FetchError::AuthRequired) => {
                    let builder = Response::builder()
                        .header(
                            hyper::header::WWW_AUTHENTICATE,
                            "Basic realm=User Visible Realm",
                        )
                        .status(hyper::StatusCode::UNAUTHORIZED);
                    return Ok(builder.body(hyper::Body::empty())?);
                }
                Err(FetchError::Other(e)) => {
                    let builder =
                        Response::builder().status(hyper::StatusCode::INTERNAL_SERVER_ERROR);
                    return Ok(builder.body(hyper::Body::from(e.0))?);
                }
            };

            parsed.execute(&root_node, &context).await
        }
    };

    let code = if res.is_ok() {
        hyper::StatusCode::OK
    } else {
        hyper::StatusCode::BAD_REQUEST
    };

    let body = hyper::Body::from(serde_json::to_string_pretty(&res).unwrap());
    let mut resp = Response::new(hyper::Body::empty());
    *resp.status_mut() = code;
    resp.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    *resp.body_mut() = body;
    let gql_result = resp;

    tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
        let temp_ns = Arc::new(josh_proxy::TmpGitNamespace::new(
            &serv.repo_path.join("overlay"),
            tracing::Span::current(),
        ));

        for (reference, oid) in context.to_push.lock()?.iter() {
            josh_proxy::push_head_url(
                context.transaction.lock()?.repo(),
                &serv
                    .repo_path
                    .join("mirror")
                    .join("objects")
                    .to_str()
                    .unwrap(),
                *oid,
                &reference,
                &remote_url,
                &remote_auth,
                &temp_ns.name(),
                "META_PUSH",
                false,
            )?;
        }
        Ok(())
    })
    .in_current_span()
    .await??;
    return Ok(gql_result);
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

    // Set format for propagating tracing context. This allows to link traces from one invocation
    // of josh to the next
    global::set_text_map_propagator(TraceContextPropagator::new());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(false)
        .with_writer(io::stderr);

    let filter = match std::env::var("RUST_LOG") {
        Ok(_) => tracing_subscriber::EnvFilter::from_default_env(),
        _ => tracing_subscriber::EnvFilter::new("josh=trace,josh_proxy=trace"),
    };

    if let Ok(endpoint) = std::env::var("JOSH_JAEGER_ENDPOINT") {
        let tracer = opentelemetry_jaeger::new_agent_pipeline()
            .with_service_name(
                std::env::var("JOSH_SERVICE_NAME").unwrap_or("josh-proxy".to_owned()),
            )
            .with_endpoint(endpoint)
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
