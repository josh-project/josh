#[macro_use]
extern crate lazy_static;
extern crate clap;

use clap::Parser;
use josh_proxy::cli;
use josh_proxy::{FetchError, MetaConfig, RemoteAuth, RepoConfig, RepoUpdate, run_git_with_auth};
use opentelemetry::global;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::Layer;

use futures::FutureExt;
use futures::future;
use hyper::body::HttpBody;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server, StatusCode};

use indoc::formatdoc;
use josh::{JoshError, JoshResult, josh_error};
use josh_rpc::calls::RequestedCommand;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::process::Command;
use tracing::{Span, trace};
use tracing_futures::Instrument;

fn version_str() -> String {
    format!("Version: {}\n", josh::VERSION,)
}

lazy_static! {
    static ref ARGS: josh_proxy::cli::Args = josh_proxy::cli::Args::parse();
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

async fn fetch_needed(
    service: Arc<JoshProxyService>,
    remote_url: &str,
    upstream_repo: &str,
    force: bool,
    head_ref: Option<&str>,
    head_ref_resolved: Option<&str>,
) -> Result<bool, FetchError> {
    let fetch_timer_ok = {
        let last = {
            let fetch_timers = service.fetch_timers.read()?;
            fetch_timers.get(remote_url).cloned()
        };

        if let Some(last) = last {
            let since = std::time::Instant::now().duration_since(last);
            let max = std::time::Duration::from_secs(ARGS.cache_duration);

            tracing::trace!("last: {:?}, since: {:?}, max: {:?}", last, since, max);
            since < max
        } else {
            false
        }
    };

    let resolve_cache_ref = |cache_ref: &str| {
        let cache_ref = cache_ref.to_string();
        let upstream_repo = upstream_repo.to_string();

        tokio::task::spawn_blocking(move || {
            let transaction = josh::cache::Transaction::open(
                &service.repo_path.join("mirror"),
                Some(&format!(
                    "refs/josh/upstream/{}/",
                    &josh::to_ns(&upstream_repo),
                )),
            )?;

            match transaction
                .repo()
                .refname_to_id(&transaction.refname(&cache_ref))
            {
                Ok(oid) => Ok(Some(oid)),
                Err(_) => Ok(None),
            }
        })
    };

    match (force, fetch_timer_ok, head_ref, head_ref_resolved) {
        (false, true, None, _) => return Ok(false),
        (false, true, Some(head_ref), _) => {
            if (resolve_cache_ref(head_ref)
                .await?
                .map_err(FetchError::from_josh_error)?)
            .is_some()
            {
                trace!("cache ref resolved");
                return Ok(false);
            }
        }
        (false, false, Some(head_ref), Some(head_ref_resolved)) => {
            if let Some(oid) = resolve_cache_ref(head_ref)
                .await?
                .map_err(FetchError::from_josh_error)?
            {
                if oid.to_string() == head_ref_resolved {
                    trace!("cache ref resolved and matches");
                    return Ok(false);
                }
            }
        }
        _ => (),
    };

    return Ok(true);
}

#[tracing::instrument(skip(service))]
async fn fetch_upstream(
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    remote_auth: &RemoteAuth,
    remote_url: String,
    head_ref: Option<&str>,
    head_ref_resolved: Option<&str>,
    force: bool,
) -> Result<(), FetchError> {
    let refs_to_fetch = match head_ref {
        Some(head_ref) if head_ref != "HEAD" && !head_ref.starts_with("refs/heads/") => {
            vec![
                "HEAD*",
                "refs/josh/*",
                "refs/heads/*",
                "refs/tags/*",
                head_ref,
            ]
        }
        _ => {
            vec!["HEAD*", "refs/josh/*", "refs/heads/*", "refs/tags/*"]
        }
    };

    let refs_to_fetch: Vec<_> = refs_to_fetch.iter().map(|x| x.to_string()).collect();

    // Check if we really need to fetch before locking the semaphore. This avoids
    // A "no fetch" case waiting for some already running fetch just to do nothing.
    if !fetch_needed(
        service.clone(),
        &remote_url,
        &upstream_repo,
        force,
        head_ref,
        head_ref_resolved,
    )
    .await?
    {
        return Ok(());
    }

    let semaphore = service
        .fetch_permits
        .lock()?
        .entry(upstream_repo.clone())
        .or_insert(Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();
    let permit = semaphore.acquire().await;

    // Check the fetch condition once again after locking the semaphore, as an unknown
    // amount of time might have passed and the outcome of this check might have changed
    // while waiting.
    if !fetch_needed(
        service.clone(),
        &remote_url,
        &upstream_repo,
        force,
        head_ref,
        head_ref_resolved,
    )
    .await?
    {
        return Ok(());
    }

    let fetch_result = {
        let span = tracing::span!(tracing::Level::INFO, "fetch_refs_from_url");

        let mirror_path = service.repo_path.join("mirror");
        let upstream_repo = upstream_repo.clone();
        let remote_url = remote_url.clone();
        let remote_auth = remote_auth.clone();

        tokio::task::spawn_blocking(move || {
            let _span_guard = span.enter();
            josh_proxy::fetch_refs_from_url(
                &mirror_path,
                &upstream_repo,
                &remote_url,
                &refs_to_fetch,
                &remote_auth,
            )
        })
        .await?
    };

    let hres = {
        let span = tracing::span!(tracing::Level::INFO, "get_head");

        let mirror_path = service.repo_path.join("mirror");
        let remote_url = remote_url.clone();
        let remote_auth = remote_auth.clone();

        tokio::task::spawn_blocking(move || {
            let _span_guard = span.enter();
            josh_proxy::get_head(&mirror_path, &remote_url, &remote_auth)
        })
        .await?
    };

    let fetch_timers = service.fetch_timers.clone();
    let heads_map = service.heads_map.clone();

    if let Ok(hres) = hres {
        heads_map.write()?.insert(upstream_repo.clone(), hres);
    }

    std::mem::drop(permit);

    if fetch_result.is_ok() {
        fetch_timers
            .write()?
            .insert(remote_url.clone(), std::time::Instant::now());
    }

    match (fetch_result, remote_auth) {
        (Ok(_), RemoteAuth::Http { auth }) => {
            if let Some((auth_user, _)) = auth.parse() {
                if matches!(&ARGS.poll_user, Some(user) if auth_user == user.as_str()) {
                    service
                        .poll
                        .lock()?
                        .insert((upstream_repo, auth.clone(), remote_url));
                }
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

async fn repo_update_fn(req: Request<hyper::Body>) -> josh::JoshResult<Response<hyper::Body>> {
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

fn resolve_ref(
    transaction: &josh::cache::Transaction,
    repo: &str,
    ref_value: &str,
) -> JoshResult<git2::Oid> {
    let josh_name = ["refs", "josh", "upstream", &josh::to_ns(repo), ref_value]
        .iter()
        .collect::<PathBuf>();

    Ok(transaction
        .repo()
        .find_reference(josh_name.to_str().unwrap())
        .map_err(|e| josh_error(&format!("Could not find ref: {}", e)))?
        .target()
        .ok_or(josh_error("Could not resolve ref"))?)
}

#[tracing::instrument(skip(service))]
async fn do_filter(
    repo_path: std::path::PathBuf,
    service: Arc<JoshProxyService>,
    meta: josh_proxy::MetaConfig,
    temp_ns: Arc<josh_proxy::TmpGitNamespace>,
    filter: josh::filter::Filter,
    head_ref: &HeadRef,
) -> josh::JoshResult<()> {
    let permit = service.filter_permits.acquire().await;
    let heads_map = service.heads_map.clone();

    let tracing_span = tracing::span!(tracing::Level::INFO, "do_filter worker");
    let head_ref = head_ref.clone();

    tokio::task::spawn_blocking(move || {
        let _span_guard = tracing_span.enter();
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

        let lazy_refs: Vec<_> = josh::filter::lazy_refs(filter)
            .iter()
            .map(|x| x.split_once("@").unwrap())
            .map(|(x, y)| (x.to_string(), y.to_string()))
            .collect();

        let resolved_refs = lazy_refs
            .iter()
            .map(|(rp, rf)| {
                (
                    format!("{}@{}", rp, rf),
                    resolve_ref(&transaction, rp, rf).unwrap(),
                )
            })
            .collect();

        let filter = josh::filter::resolve_refs(&resolved_refs, filter);

        let (refs_list, head_ref) = match &head_ref {
            HeadRef::Explicit(ref_value)
                if ref_value.starts_with("refs/") || ref_value == "HEAD" =>
            {
                let object = resolve_ref(&transaction, &meta.config.repo, &ref_value)?;
                let list = vec![(ref_value.clone(), object)];

                (list, ref_value.clone())
            }
            HeadRef::Explicit(ref_value) => {
                // When it's not something starting with refs/ or HEAD, it's
                // probably sha1
                let list = vec![(ref_value.to_string(), git2::Oid::from_str(&ref_value)?)];

                let synthetic_ref = format!("refs/heads/_{}", ref_value);
                (list, synthetic_ref)
            }
            HeadRef::Implicit => {
                // When user did not explicitly request a ref to filter,
                // start with a list of all existing refs
                let mut list =
                    josh::housekeeping::list_refs(transaction.repo(), &meta.config.repo)?;

                let head_ref = head_ref.get().to_string();
                if let Ok(object) = resolve_ref(&transaction, &meta.config.repo, &head_ref) {
                    list.push((head_ref.clone(), object));
                }

                (list, head_ref)
            }
        };

        let head_ref = if head_ref == "HEAD" {
            heads_map
                .read()?
                .get(&meta.config.repo)
                .unwrap_or(&"invalid".to_string())
                .clone()
        } else {
            head_ref
        };

        let t2 = josh::cache::Transaction::open(&repo_path.join("overlay"), None)?;
        t2.repo()
            .odb()?
            .add_disk_alternate(repo_path.join("mirror").join("objects").to_str().unwrap())?;
        let (updated_refs, _) = josh::filter_refs(&t2, filter, &refs_list, josh::filter::empty());
        let mut updated_refs = josh_proxy::refs_locking(updated_refs, &meta);
        josh::housekeeping::namespace_refs(&mut updated_refs, temp_ns.name());
        josh::update_refs(&t2, &mut updated_refs, &temp_ns.reference(&head_ref));
        t2.repo()
            .reference_symbolic(
                &temp_ns.reference("HEAD"),
                &temp_ns.reference(&head_ref),
                true,
                "",
            )
            .ok();

        Ok::<_, JoshError>(())
    })
    .await??;

    std::mem::drop(permit);

    Ok(())
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

    Ok(response)
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
        remote_auth,
        remote_url.to_owned(),
        Some("HEAD"),
        None,
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
        Some(&format!("refs/josh/upstream/{}/", &josh::to_ns(meta_repo),)),
    )?;

    let meta_tree = transaction
        .repo()
        .find_reference(&transaction.refname("HEAD"))?
        .peel_to_tree()?;

    let meta_blob = josh::filter::tree::get_blob(
        transaction.repo(),
        &meta_tree,
        &std::path::Path::new(&upstream_repo.trim_start_matches('/')).join("config.yml"),
    );

    if meta_blob.is_empty() {
        return Err(josh::josh_error("meta repo entry not found"));
    }

    let mut meta: josh_proxy::MetaConfig = Default::default();

    meta.config = serde_yaml::from_str(&meta_blob)?;

    if meta.config.lock_refs {
        let meta_blob = josh::filter::tree::get_blob(
            transaction.repo(),
            &meta_tree,
            &std::path::Path::new(&upstream_repo.trim_start_matches('/')).join("lock.yml"),
        );

        if meta_blob.is_empty() {
            return Err(josh::josh_error("locked refs not found"));
        }
        meta.refs_lock = serde_yaml::from_str(&meta_blob)?;
    }

    Ok(meta)
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

async fn ssh_list_refs(
    url: &str,
    auth_socket: std::path::PathBuf,
    refs: Option<&[&str]>,
) -> JoshResult<HashMap<String, String>> {
    let temp_dir = tempfile::TempDir::with_prefix("josh")?;
    let refs = match refs {
        Some(refs) => refs.to_vec(),
        None => vec!["HEAD"],
    };

    let ls_remote = vec!["git", "ls-remote", url];
    let command = ls_remote
        .iter()
        .chain(refs.iter())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let result = tokio::task::spawn_blocking(move || -> JoshResult<(String, String, i32)> {
        let command = command.iter().map(String::as_str).collect::<Vec<_>>();

        let (stdout, stderr, code) = run_git_with_auth(
            temp_dir.path(),
            &command,
            &RemoteAuth::Ssh { auth_socket },
            None,
        )?;

        Ok((stdout, stderr, code))
    })
    .await?;

    let stdout = match result {
        Ok((stdout, _, 0)) => stdout,
        Ok((_, stderr, code)) => {
            return Err(josh_error(&format!(
                "auth check: git exited with code {}: {}",
                code, stderr
            )));
        }
        Err(e) => return Err(e),
    };

    let refs = stdout
        .lines()
        .map(|line| {
            match line
                .split('\t')
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .as_slice()
            {
                [sha1, git_ref] => Ok((git_ref.to_owned(), sha1.to_owned())),
                _ => Err(josh_error("could not parse result of ls-remote")),
            }
        })
        .collect::<JoshResult<HashMap<_, _>>>()?;

    Ok(refs)
}

async fn serve_namespace(
    params: &josh_rpc::calls::ServeNamespace,
    repo_path: std::path::PathBuf,
    namespace: &str,
    repo_update: RepoUpdate,
) -> josh::JoshResult<()> {
    const SERVE_TIMEOUT: u64 = 60;

    tracing::trace!(
        command = ?params.command,
        query = %params.query,
        namespace = %namespace,
        "serve_namespace",
    );

    enum ServeError {
        FifoError(std::io::Error),
        SubprocessError(std::io::Error),
        SubprocessTimeout(tokio::time::error::Elapsed),
        SubprocessExited(i32),
    }

    let command = match params.command {
        RequestedCommand::GitUploadPack => "git-upload-pack",
        RequestedCommand::GitUploadArchive => "git-upload-archive",
        RequestedCommand::GitReceivePack => "git-receive-pack",
    };

    let overlay_path = repo_path.join("overlay");

    let mut process = tokio::process::Command::new(command)
        .arg(&overlay_path)
        .current_dir(&overlay_path)
        .env("GIT_DIR", &repo_path)
        .env("GIT_NAMESPACE", namespace)
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            repo_path.join("mirror").join("objects"),
        )
        .env("JOSH_REPO_UPDATE", serde_json::to_string(&repo_update)?)
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

        copy_future.await.map_err(ServeError::FifoError)
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
                    .map_err(ServeError::FifoError)
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

fn is_repo_blocked(config: &RepoConfig) -> bool {
    let block = std::env::var("JOSH_REPO_BLOCK").unwrap_or("".to_owned());
    let block = block.split(';').collect::<Vec<_>>();

    for b in block {
        if b == config.repo {
            return true;
        }
    }

    false
}

#[derive(Clone, Debug)]
enum HeadRef {
    Explicit(String),
    Implicit,
}

impl HeadRef {
    // Sometimes we don't care about whether it's implicit or explicit
    fn get(&self) -> &str {
        match self {
            HeadRef::Explicit(r) => &r,
            HeadRef::Implicit => "HEAD",
        }
    }
}

fn head_ref_or_default(head_ref: &str) -> HeadRef {
    let result = head_ref
        .trim_start_matches(|char| char == '@' || char == '^')
        .to_owned();

    if result.is_empty() {
        HeadRef::Implicit
    } else {
        HeadRef::Explicit(result)
    }
}

fn make_repo_update(
    remote_url: &str,
    serv: Arc<JoshProxyService>,
    filter: josh::filter::Filter,
    remote_auth: RemoteAuth,
    meta: &MetaConfig,
    repo_path: &Path,
    ns: Arc<josh_proxy::TmpGitNamespace>,
) -> RepoUpdate {
    let context_propagator = josh_proxy::trace::make_context_propagator();

    RepoUpdate {
        refs: HashMap::new(),
        remote_url: remote_url.to_string(),
        remote_auth,
        port: serv.port.clone(),
        filter_spec: josh::filter::spec(filter),
        base_ns: josh::to_ns(&meta.config.repo),
        git_ns: ns.name().to_string(),
        git_dir: repo_path.display().to_string(),
        mirror_git_dir: serv.repo_path.join("mirror").display().to_string(),
        context_propagator,
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
            ));
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

    eprintln!("params: {:?}", params);
    eprintln!("parsed_url.upstream_repo: {:?}", parsed_url.upstream_repo);

    let auth_socket = params.ssh_socket.clone();
    let remote_auth = RemoteAuth::Ssh {
        auth_socket: auth_socket.clone(),
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

    if is_repo_blocked(&meta_config.config) {
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
            ));
        }
    };

    let remote_url = upstream + meta_config.config.repo.as_str();
    let head_ref = head_ref_or_default(&parsed_url.headref);

    let resolved_ref = match params.command {
        // When pushing over SSH, we need to fetch to get new references
        // for searching for unapply base, so we don't bother with additional cache checks
        RequestedCommand::GitReceivePack => None,
        // Otherwise, list refs - it doesn't need locking and is faster -
        // and use results to potentially skip fetching
        _ => {
            let remote_refs = [head_ref.get()];
            let remote_refs =
                match ssh_list_refs(&remote_url, auth_socket, Some(&remote_refs)).await {
                    Ok(remote_refs) => remote_refs,
                    Err(e) => {
                        return Ok(make_response(
                            hyper::Body::from(e.to_string()),
                            hyper::StatusCode::FORBIDDEN,
                        ));
                    }
                };

            match remote_refs.get(head_ref.get()) {
                Some(resolved_ref) => Some(resolved_ref.clone()),
                None => {
                    return Ok(make_response(
                        hyper::Body::from("Could not resolve remote ref"),
                        hyper::StatusCode::INTERNAL_SERVER_ERROR,
                    ));
                }
            }
        }
    };

    match fetch_upstream(
        serv.clone(),
        meta_config.config.repo.to_owned(),
        &remote_auth,
        remote_url.to_owned(),
        Some(head_ref.get()),
        resolved_ref.as_deref(),
        false,
    )
    .await
    {
        Ok(_) => {}
        Err(FetchError::AuthRequired) => {
            return Ok(make_response(
                hyper::Body::from("Access to upstream repo denied"),
                StatusCode::FORBIDDEN,
            ));
        }
        Err(FetchError::Other(e)) => {
            return Ok(make_response(
                hyper::Body::from(e.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
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
                hyper::Body::from(format!("Failed to parse filter: {}", e)),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let filter = josh::filter::chain(
        query_filter,
        match &ARGS.filter_prefix {
            Some(filter_prefix) => {
                let filter_prefix = match josh::filter::parse(filter_prefix) {
                    Ok(filter) => filter,
                    Err(e) => {
                        return Ok(make_response(
                            hyper::Body::from(format!(
                                "Failed to parse prefix filter passed as command line argument: {}",
                                e
                            )),
                            StatusCode::SERVICE_UNAVAILABLE,
                        ));
                    }
                };

                josh::filter::chain(meta_config.config.filter, filter_prefix)
            }
            None => meta_config.config.filter,
        },
    );

    let temp_ns = match prepare_namespace(serv.clone(), &meta_config, filter, &head_ref).await {
        Ok(ns) => ns,
        Err(e) => {
            return Ok(make_response(
                hyper::Body::from(e.to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let overlay_path = serv.repo_path.join("overlay");
    let repo_update = make_repo_update(
        &remote_url,
        serv.clone(),
        filter,
        remote_auth,
        &meta_config,
        &overlay_path,
        temp_ns.clone(),
    );

    let serve_result =
        serve_namespace(&params, serv.repo_path.clone(), temp_ns.name(), repo_update).await;
    std::mem::drop(temp_ns);

    match serve_result {
        Ok(_) => Ok(make_response(hyper::Body::empty(), StatusCode::NO_CONTENT)),
        Err(e) => Ok(make_response(
            hyper::Body::from(e.to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

#[tracing::instrument(skip(serv))]
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
        percent_encoding::percent_decode_str(&path)
            .decode_utf8_lossy()
            .to_string()
    };

    if let Some(resource_path) = path.strip_prefix("/~/ui") {
        return handle_ui_request(req, resource_path).await;
    }

    if let Some(response) = static_paths(&serv, &path).await? {
        return Ok(response);
    }

    // When exposed to internet, should be blocked
    if path == "/repo_update" {
        return repo_update_fn(req).await;
    }

    if path == "/serve_namespace" {
        return handle_serve_namespace_request(serv.clone(), req).await;
    }

    // Need to have some way of passing the filter (via remote path like what github does?)
    let parsed_url = {
        if let Some(parsed_url) = FilteredRepoUrl::from_str(&path) {
            let mut pu = parsed_url;

            if pu.rest.starts_with(':') {
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
            ));
        }
    };
    if let Some(filter_prefix) = &ARGS.filter_prefix {
        filter = josh::filter::chain(josh::filter::parse(filter_prefix)?, filter);
    }
    let mut fetch_repos = vec![meta.config.repo.clone()];

    let lazy_refs: Vec<_> = josh::filter::lazy_refs(filter)
        .iter()
        .map(|x| x.split_once("@").unwrap())
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect();

    fetch_repos.extend(lazy_refs.iter().map(|(x, _y)| x.clone()));

    ///////////////

    let remote_url = upstream.clone() + meta.config.repo.as_str();
    let headref = head_ref_or_default(&parsed_url.headref);

    if parsed_url.pathinfo.starts_with("/info/lfs") {
        return Ok(Response::builder()
            .status(hyper::StatusCode::TEMPORARY_REDIRECT)
            .header("Location", format!("{}{}", remote_url, parsed_url.pathinfo))
            .body(hyper::Body::empty())?);
    }

    let http_auth_required = ARGS.require_auth && parsed_url.pathinfo == "/git-receive-pack";

    for fetch_repo in fetch_repos.iter() {
        let fetch_url = upstream.clone() + fetch_repo.as_str();
        if is_repo_blocked(&meta.config) {
            return Ok(make_response(
                hyper::Body::from(formatdoc!(
                    r#"
                        Access to this repo is blocked via JOSH_REPO_BLOCK
                        "#
                )),
                hyper::StatusCode::UNPROCESSABLE_ENTITY,
            ));
        }

        if !josh_proxy::auth::check_http_auth(&fetch_url, &auth, http_auth_required).await? {
            tracing::trace!("require-auth");
            let builder = Response::builder()
                .header(
                    hyper::header::WWW_AUTHENTICATE,
                    "Basic realm=User Visible Realm",
                )
                .status(hyper::StatusCode::UNAUTHORIZED);
            return Ok(builder.body(hyper::Body::empty())?);
        }
    }

    if parsed_url.api == "/~/graphql" {
        return serve_graphql(serv, req, meta.config.repo.to_owned(), upstream, auth).await;
    }

    if parsed_url.api == "/~/graphiql" {
        let addr = format!("/~/graphql{}", meta.config.repo);
        return Ok(tokio::task::spawn_blocking(move || {
            josh_proxy::juniper_hyper::graphiql(&addr, None)
        })
        .in_current_span()
        .await??);
    }

    for fetch_repo in fetch_repos.iter() {
        let fetch_url = upstream.clone() + fetch_repo.as_str();
        match fetch_upstream(
            serv.clone(),
            fetch_repo.to_owned(),
            &remote_auth,
            fetch_url.to_owned(),
            Some(headref.get()),
            None,
            false,
        )
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
    }

    //////////////

    if let (Some(q), true) = (
        req.uri().query().map(|x| x.to_string()),
        parsed_url.pathinfo.is_empty(),
    ) {
        return serve_query(serv, q, meta.config.repo, filter, headref.get()).await;
    }

    let temp_ns = prepare_namespace(serv.clone(), &meta, filter, &headref).await?;
    let overlay_path = serv.repo_path.join("overlay");

    let repo_update = make_repo_update(
        &remote_url,
        serv.clone(),
        filter,
        remote_auth,
        &meta,
        &overlay_path,
        temp_ns.clone(),
    );

    let cgi_response = async {
        let mut cmd = Command::new("git");
        cmd.arg("http-backend");
        cmd.current_dir(&overlay_path);
        cmd.env("GIT_DIR", &overlay_path);
        cmd.env("GIT_HTTP_EXPORT_ALL", "");
        cmd.env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            serv.repo_path
                .join("mirror")
                .join("objects")
                .display()
                .to_string(),
        );
        cmd.env("GIT_NAMESPACE", temp_ns.name());
        cmd.env("GIT_PROJECT_ROOT", &overlay_path);
        cmd.env("JOSH_REPO_UPDATE", serde_json::to_string(&repo_update)?);
        cmd.env("PATH_INFO", parsed_url.pathinfo.clone());

        let (response, stderr) = hyper_cgi::do_cgi(req, cmd).await;
        tracing::debug!(stderr = %String::from_utf8_lossy(&stderr), "http-backend exited");

        Ok::<_, JoshError>(response)
    }
    .instrument(tracing::span!(
        tracing::Level::INFO,
        "hyper_cgi / git-http-backend"
    ))
    .await?;

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    std::mem::drop(temp_ns);

    Ok(cgi_response)
}

async fn serve_query(
    serv: Arc<JoshProxyService>,
    q: String,
    upstream_repo: String,
    filter: josh::filter::Filter,
    head_ref: &str,
) -> josh::JoshResult<Response<hyper::Body>> {
    let tracing_span = tracing::span!(tracing::Level::TRACE, "render worker");
    let head_ref = head_ref.to_string();
    let res = tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
        let _span_guard = tracing_span.enter();

        let transaction_mirror = josh::cache::Transaction::open(
            &serv.repo_path.join("mirror"),
            Some(&format!(
                "refs/josh/upstream/{}/",
                &josh::to_ns(&upstream_repo),
            )),
        )?;

        let transaction = josh::cache::Transaction::open(&serv.repo_path.join("overlay"), None)?;
        transaction.repo().odb()?.add_disk_alternate(
            serv.repo_path
                .join("mirror")
                .join("objects")
                .to_str()
                .unwrap(),
        )?;

        let commit_id = if let Ok(oid) = git2::Oid::from_str(&head_ref) {
            oid
        } else {
            transaction_mirror
                .repo()
                .refname_to_id(&transaction_mirror.refname(&head_ref))?
        };
        let commit_id =
            josh::filter_commit(&transaction, filter, commit_id, josh::filter::empty())?;

        josh_templates::render(&transaction, "", commit_id, &q, true)
    })
    .in_current_span()
    .await?;

    Ok(match res {
        Ok(Some((res, params))) => Response::builder()
            .status(hyper::StatusCode::OK)
            .header(
                "content-type",
                params
                    .get("content-type")
                    .unwrap_or(&"text/plain".to_string()),
            )
            .body(hyper::Body::from(res))?,

        Ok(None) => Response::builder()
            .status(hyper::StatusCode::NOT_FOUND)
            .body(hyper::Body::from("File not found".to_string()))?,

        Err(res) => Response::builder()
            .status(hyper::StatusCode::UNPROCESSABLE_ENTITY)
            .body(hyper::Body::from(res.to_string()))?,
    })
}

#[tracing::instrument(skip(serv))]
async fn prepare_namespace(
    serv: Arc<JoshProxyService>,
    meta: &josh_proxy::MetaConfig,
    filter: josh::filter::Filter,
    head_ref: &HeadRef,
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
        head_ref,
    )
    .await?;

    Ok(temp_ns)
}

fn trace_http_response(trace_span: Span, response: &Response<hyper::Body>) {
    use opentelemetry_semantic_conventions::trace::HTTP_RESPONSE_STATUS_CODE;

    macro_rules! trace {
        ($level:expr) => {{
            tracing::event!(
                parent: trace_span,
                $level,
                { HTTP_RESPONSE_STATUS_CODE } = response.status().as_u16()
            );
        }};
    }

    match response.status().as_u16() {
        s if s < 400 => trace!(tracing::Level::TRACE),
        s if s < 500 => trace!(tracing::Level::WARN),
        _ => trace!(tracing::Level::ERROR),
    };
}

/// Turn a list of [cli::Remote] into a [JoshProxyUpstream] struct.
fn make_upstream(remotes: &Vec<cli::Remote>) -> josh::JoshResult<JoshProxyUpstream> {
    if remotes.is_empty() {
        unreachable!() // already checked in the parser
    } else if remotes.len() == 1 {
        Ok(match &remotes[0] {
            cli::Remote::Http(url) => JoshProxyUpstream::Http(url.to_string()),
            cli::Remote::Ssh(url) => JoshProxyUpstream::Ssh(url.to_string()),
        })
    } else if remotes.len() == 2 {
        Ok(match (&remotes[0], &remotes[1]) {
            (cli::Remote::Http(_), cli::Remote::Http(_))
            | (cli::Remote::Ssh(_), cli::Remote::Ssh(_)) => {
                return Err(josh_error("two cli::remotes of the same type passed"));
            }
            (cli::Remote::Http(http_url), cli::Remote::Ssh(ssh_url))
            | (cli::Remote::Ssh(ssh_url), cli::Remote::Http(http_url)) => JoshProxyUpstream::Both {
                http: http_url.to_string(),
                ssh: ssh_url.to_string(),
            },
        })
    } else {
        Err(josh_error("too many remotes"))
    }
}

#[tracing::instrument(skip_all, fields(url.path, http.request.method))]
async fn handle_http_request(
    proxy_service: Arc<JoshProxyService>,
    req: Request<hyper::body::Body>,
) -> Result<Response<hyper::Body>, hyper::http::Error> {
    use opentelemetry_semantic_conventions::trace::{HTTP_REQUEST_METHOD, URL_PATH};

    let span = tracing::Span::current();
    span.record(URL_PATH, req.uri().path());
    span.record(HTTP_REQUEST_METHOD, req.method().to_string());

    async move {
        let response = if let Ok(req_auth) = josh_proxy::auth::strip_auth(req) {
            call_service(proxy_service, req_auth)
                .await
                .unwrap_or_else(|e| {
                    make_response(
                        hyper::Body::from(match e {
                            JoshError(s) => s,
                        }),
                        hyper::StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })
        } else {
            make_response(
                hyper::Body::from("JoshError(strip_auth)"),
                hyper::StatusCode::INTERNAL_SERVER_ERROR,
            )
        };

        trace_http_response(span.clone(), &response);
        response
    }
    .map(Ok::<_, hyper::http::Error>)
    .await
}

#[tokio::main]
async fn run_proxy() -> josh::JoshResult<i32> {
    init_trace();

    let addr = format!("[::]:{}", ARGS.port).parse()?;
    let upstream = make_upstream(&ARGS.remote).map_err(|e| {
        eprintln!("Upstream parsing error: {}", &e);
        e
    })?;

    let local = std::path::PathBuf::from(&ARGS.local.as_ref().unwrap());
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
        let service = service_fn(move |req| {
            let proxy_service = proxy_service.clone();
            handle_http_request(proxy_service, req)
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
                None,
                None,
                true,
            )
            .in_current_span()
            .await;

            match fetch_result {
                Ok(()) => {}
                Err(FetchError::Other(e)) => return Err(e),
                Err(FetchError::AuthRequired) => {
                    return Err(josh_error("auth: access denied while polling"));
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

fn repo_update_from_env() -> josh::JoshResult<josh_proxy::RepoUpdate> {
    let repo_update =
        std::env::var("JOSH_REPO_UPDATE").map_err(|_| josh_error("JOSH_REPO_UPDATE not set"))?;

    serde_json::from_str(&repo_update)
        .map_err(|e| josh_error(&format!("Failed to parse JOSH_REPO_UPDATE: {}", e)))
}

fn pre_receive_hook() -> josh::JoshResult<i32> {
    let repo_update = repo_update_from_env()?;

    let push_options_path = std::path::PathBuf::from(repo_update.git_dir)
        .join("refs/namespaces")
        .join(repo_update.git_ns)
        .join("push_options");

    let push_option_count: usize = std::env::var("GIT_PUSH_OPTION_COUNT")?.parse()?;

    let mut push_options = HashMap::<String, serde_json::Value>::new();
    for i in 0..push_option_count {
        let push_option = std::env::var(format!("GIT_PUSH_OPTION_{}", i))?;
        if let Some((key, value)) = push_option.split_once("=") {
            push_options.insert(key.into(), value.into());
        } else {
            push_options.insert(push_option, true.into());
        }
    }

    std::fs::write(push_options_path, serde_json::to_string(&push_options)?)?;

    Ok(0)
}

fn update_hook(refname: &str, old: &str, new: &str) -> josh::JoshResult<i32> {
    let mut repo_update = repo_update_from_env()?;

    repo_update
        .refs
        .insert(refname.to_owned(), (old.to_owned(), new.to_owned()));

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let resp = client
        .post(format!("http://localhost:{}/repo_update", repo_update.port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(resp) => {
            let success = resp.status().is_success();
            println!("upstream: response status: {}", resp.status());

            match resp.text() {
                Ok(text) if text.trim().is_empty() => {
                    println!("upstream: no response body");
                }
                Ok(text) => {
                    println!("upstream: response body:\n\n{}", text);
                }
                Err(err) => {
                    println!("upstream: warn: failed to read response body: {:?}", err);
                }
            }

            if success { Ok(0) } else { Ok(1) }
        }
        Err(err) => {
            tracing::warn!("/repo_update request failed {:?}", err);
            Ok(1)
        }
    }
}

#[tracing::instrument(skip_all)]
async fn serve_graphql(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
    upstream_repo: String,
    upstream: String,
    auth: josh_proxy::auth::Handle,
) -> josh::JoshResult<Response<hyper::Body>> {
    let remote_url = upstream.clone() + upstream_repo.as_str();
    let parsed_request = match josh_proxy::juniper_hyper::parse_req(req).await {
        Ok(parsed_request) => {
            // Even though there's a mutex, it's just to manage access
            // between sync and async code, so no contention is expected
            Arc::new(std::sync::Mutex::new(parsed_request))
        }
        Err(resp) => return Ok(resp),
    };

    let context = {
        let upstream_repo = upstream_repo.clone();
        let serv = serv.clone();

        tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
            let transaction_mirror = josh::cache::Transaction::open(
                &serv.repo_path.join("mirror"),
                Some(&format!(
                    "refs/josh/upstream/{}/",
                    &josh::to_ns(&upstream_repo),
                )),
            )?;

            let transaction =
                josh::cache::Transaction::open(&serv.repo_path.join("overlay"), None)?;
            transaction.repo().odb()?.add_disk_alternate(
                &serv
                    .repo_path
                    .join("mirror")
                    .join("objects")
                    .display()
                    .to_string(),
            )?;

            Ok(Arc::new(josh_graphql::graphql::context(
                transaction,
                transaction_mirror,
            )))
        })
        .await??
    };

    let root_node = Arc::new(josh_graphql::graphql::repo_schema(
        upstream_repo
            .strip_suffix(".git")
            .unwrap_or(&upstream_repo)
            .to_string(),
        false,
    ));

    let run_request = |span: tracing::Span| {
        let context = context.clone();
        let parsed_request = parsed_request.clone();
        let root_node = root_node.clone();

        tokio::task::spawn_blocking(move || {
            let _span_guard = span.enter();

            let parsed_request = parsed_request.lock().unwrap();
            let result = parsed_request.execute_sync(&root_node, &context);

            let response_code = if result.is_ok() {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            };

            let response_json = serde_json::to_string_pretty(&result)
                .expect("bug: failed to serialize GraphQL response");

            (response_code, response_json)
        })
    };

    let remote_auth = RemoteAuth::Http { auth };
    let (response_code, response_json) = {
        // First attempt to serve GraphQL query. If we can serve it
        // that means all requested revisions were specified by SHA and we could find
        // all of them locally, so no need to fetch.
        let execute_span = tracing::info_span!("execute_1");
        let (response_code, response_json) = run_request(execute_span).await?;

        // The "allow_refs" flag will be set by the query handler if we need to do a fetch
        // to complete the query.
        if !*context.allow_refs.lock().unwrap() {
            (response_code, response_json)
        } else {
            match fetch_upstream(
                serv.clone(),
                upstream_repo.to_owned(),
                &remote_auth,
                remote_url.to_owned(),
                Some("HEAD"),
                None,
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

            let execute_span = tracing::info_span!("execute_2");
            run_request(execute_span).await?
        }
    };

    let response = {
        let mut response = Response::new(hyper::Body::from(response_json));
        *response.status_mut() = response_code;
        response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("application/json"),
        );
        response
    };

    tokio::task::spawn_blocking(move || -> josh::JoshResult<_> {
        let temp_ns = Arc::new(josh_proxy::TmpGitNamespace::new(
            &serv.repo_path.join("overlay"),
            tracing::Span::current(),
        ));

        let transaction = &*context.transaction.lock()?;
        let mut to_push = context.to_push.lock()?.clone();

        if let Some((refname, oid)) = josh_proxy::merge_meta(
            transaction,
            &*context.transaction_mirror.lock()?,
            &*context.meta_add.lock()?,
        )? {
            to_push.insert((oid, refname, None));
        }

        for (oid, refname, repo) in to_push {
            let url = if let Some(repo) = repo {
                format!("{}/{}", upstream, repo)
            } else {
                remote_url.clone()
            };
            josh_proxy::push_head_url(
                transaction.repo(),
                serv.repo_path
                    .join("mirror")
                    .join("objects")
                    .to_str()
                    .unwrap(),
                oid,
                &refname,
                &url,
                &remote_auth,
                temp_ns.name(),
                "QUERY_PUSH",
                false,
            )?;
        }

        Ok(())
    })
    .in_current_span()
    .await??;

    Ok(response)
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    println!("shutdown_signal");
}

#[allow(deprecated)]
fn init_trace() {
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::propagation::TraceContextPropagator;

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

    let service_name = std::env::var("JOSH_SERVICE_NAME").unwrap_or("josh-proxy".to_owned());

    if let Ok(endpoint) = std::env::var("JOSH_JAEGER_ENDPOINT") {
        let tracer = opentelemetry_jaeger::new_agent_pipeline()
            .with_service_name(service_name)
            .with_endpoint(endpoint)
            .install_simple()
            .expect("can't install opentelemetry pipeline");

        let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = filter
            .and_then(fmt_layer)
            .and_then(telemetry_layer)
            .with_subscriber(tracing_subscriber::Registry::default());

        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");
    } else if let Ok(endpoint) = std::env::var("JOSH_OTLP_ENDPOINT") {
        use opentelemetry::KeyValue;

        let resource =
            opentelemetry_sdk::Resource::new(vec![KeyValue::new("service.name", service_name)]);

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(endpoint),
            )
            .with_trace_config(opentelemetry_sdk::trace::config().with_resource(resource))
            .install_batch(opentelemetry_sdk::runtime::Tokio)
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
}

fn main() {
    // josh-proxy creates a symlink to itself as a git update hook.
    // When it gets called by git as that hook, the binary name will end
    // end in "/update" and this will not be a new server.
    // The update hook will then make a http request back to the main
    // process to do the actual computation while taking advantage of the
    // cached data already loaded into the main process's memory.
    if let [a0, a1, a2, a3, ..] = &std::env::args().collect::<Vec<_>>().as_slice() {
        if a0.ends_with("/update") {
            std::process::exit(update_hook(a1, a2, a3).unwrap_or(1));
        }
    }

    if let [a0, ..] = &std::env::args().collect::<Vec<_>>().as_slice() {
        if a0.ends_with("/pre-receive") {
            eprintln!("josh-proxy: pre-receive hook");
            let code = match pre_receive_hook() {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("josh-proxy: pre-receive hook failed: {}", e);
                    std::process::exit(1);
                }
            };

            std::process::exit(code);
        }
    }

    let exit_code = run_proxy().unwrap_or(1);
    global::shutdown_tracer_provider();
    std::process::exit(exit_code);
}
