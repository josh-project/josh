use josh_core::cache::CacheStack;
use josh_core::{JoshError, JoshResult, josh_error};
use josh_graphql::graphql;
use josh_proxy::http::ProxyError;
use josh_proxy::service::{JoshProxyService, UpstreamProtocol, make_upstream};
use josh_proxy::{FetchError, MetaConfig, RemoteAuth, RepoConfig, RepoUpdate, run_git_with_auth};
use josh_rpc::calls::RequestedCommand;

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use clap::Parser;
use indoc::formatdoc;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing::{Span, trace};
use tracing_futures::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use axum_extra::response::ErasedJson;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

josh_core::regex_parsed!(
    FilteredRepoUrl,
    r"(?P<api>/~/\w+)?(?P<upstream_repo>/[^:!]*[.]git)(?P<headref>[\^@][^:!]*)?((?P<filter_spec>[:!].*)[.]git)?(?P<pathinfo>/.*)?(?P<rest>.*)",
    [api, upstream_repo, filter_spec, pathinfo, headref, rest]
);

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
            let max = std::time::Duration::from_secs(service.cache_duration);

            tracing::trace!("last: {:?}, since: {:?}, max: {:?}", last, since, max);
            since < max
        } else {
            false
        }
    };

    let resolve_cache_ref = |cache_ref: &str| {
        let cache_ref = cache_ref.to_string();
        let upstream_repo = upstream_repo.to_string();
        let service = service.clone();

        tokio::task::spawn_blocking(move || {
            let transaction = service.open_mirror(Some(&format!(
                "refs/josh/upstream/{}/",
                &josh_core::to_ns(&upstream_repo)
            )))?;

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

    Ok(true)
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
                if matches!(&service.poll_user, Some(user) if auth_user == user.as_str()) {
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

async fn handle_version() -> impl IntoResponse {
    format!("Version: {}\n", josh_core::VERSION)
}

async fn handle_remote(State(service): State<Arc<JoshProxyService>>) -> impl IntoResponse {
    match service.upstream.get(UpstreamProtocol::Http) {
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTP remote is not configured",
        )
            .into_response(),
        Some(remote) => (StatusCode::OK, remote).into_response(),
    }
}

async fn handle_flush(State(service): State<Arc<JoshProxyService>>) -> impl IntoResponse {
    match service.fetch_timers.write() {
        Ok(mut timers) => {
            timers.clear();
            (StatusCode::OK, "Flushed credential cache")
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to flush cache"),
    }
}

async fn handle_filters(service: Arc<JoshProxyService>, refresh: bool) -> impl IntoResponse {
    // Clear fetch timers
    if let Err(_) = service.fetch_timers.write().map(|mut t| t.clear()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to clear fetch timers",
        )
            .into_response();
    }

    let body_str = match tokio::task::spawn_blocking(move || -> josh_core::JoshResult<_> {
        let transaction_mirror = service.open_mirror(None)?;
        josh_core::housekeeping::discover_filter_candidates(&transaction_mirror)?;

        if refresh {
            let transaction_overlay = service.open_overlay(None)?;
            josh_core::housekeeping::refresh_known_filters(
                &transaction_mirror,
                &transaction_overlay,
            )?;
        }

        Ok(toml::to_string_pretty(
            &josh_core::housekeeping::get_known_filters()?,
        )?)
    })
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    (StatusCode::OK, body_str).into_response()
}

#[tracing::instrument]
async fn handle_repo_update(
    State(_serv): State<Arc<JoshProxyService>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let body = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to read body: {}", e),
            )
                .into_response();
        }
    };

    let s = tracing::span!(tracing::Level::TRACE, "repo update worker");

    let result = tokio::task::spawn_blocking(move || {
        use opentelemetry::global;

        let _e = s.enter();
        let buffer = std::str::from_utf8(&body)?;
        let repo_update: RepoUpdate = serde_json::from_str(buffer)?;
        let context_propagator = repo_update.context_propagator.clone();
        let parent_context =
            global::get_text_map_propagator(|propagator| propagator.extract(&context_propagator));
        let _ = s.set_parent(parent_context);

        josh_proxy::process_repo_update(repo_update)
    })
    .instrument(Span::current())
    .await;

    match result {
        Ok(Ok(stderr)) => (StatusCode::OK, stderr).into_response(),
        Ok(Err(josh_core::JoshError(stderr))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, stderr).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Task error: {}", e),
        )
            .into_response(),
    }
}

fn resolve_ref(
    transaction: &josh_core::cache::Transaction,
    repo: &str,
    ref_value: &str,
) -> JoshResult<git2::Oid> {
    let josh_name = [
        "refs",
        "josh",
        "upstream",
        &josh_core::to_ns(repo),
        ref_value,
    ]
    .iter()
    .collect::<PathBuf>();

    transaction
        .repo()
        .find_reference(josh_name.to_str().unwrap())
        .map_err(|e| josh_error(&format!("Could not find ref: {}", e)))?
        .target()
        .ok_or(josh_error("Could not resolve ref"))
}

#[tracing::instrument(skip(service))]
async fn do_filter(
    repo_path: std::path::PathBuf,
    service: Arc<JoshProxyService>,
    meta: josh_proxy::MetaConfig,
    temp_ns: Arc<josh_proxy::TmpGitNamespace>,
    filter: josh_core::filter::Filter,
    head_ref: &HeadRef,
) -> josh_core::JoshResult<()> {
    let permit = service.filter_permits.acquire().await;
    let heads_map = service.heads_map.clone();

    let tracing_span = tracing::span!(tracing::Level::INFO, "do_filter worker");
    let head_ref = head_ref.clone();
    let service = service.clone();

    tokio::task::spawn_blocking(move || {
        let _span_guard = tracing_span.enter();
        tracing::trace!("in do_filter worker");
        let filter_spec = josh_core::filter::spec(filter);
        josh_core::housekeeping::remember_filter(&meta.config.repo, &filter_spec);

        let transaction = service.open_mirror(Some(&format!(
            "refs/josh/upstream/{}/",
            &josh_core::to_ns(&meta.config.repo)
        )))?;

        let lazy_refs: Vec<_> = josh_core::filter::lazy_refs(filter)
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

        let filter = josh_core::filter::resolve_refs(&resolved_refs, filter);

        let (refs_list, head_ref) = match &head_ref {
            HeadRef::Explicit(ref_value)
                if ref_value.starts_with("refs/") || ref_value == "HEAD" =>
            {
                let object = resolve_ref(&transaction, &meta.config.repo, ref_value)?;
                let list = vec![(ref_value.clone(), object)];

                (list, ref_value.clone())
            }
            HeadRef::Explicit(ref_value) => {
                // When it's not something starting with refs/ or HEAD, it's
                // probably sha1
                let list = vec![(ref_value.to_string(), git2::Oid::from_str(ref_value)?)];

                let synthetic_ref = format!("refs/heads/_{}", ref_value);
                (list, synthetic_ref)
            }
            HeadRef::Implicit => {
                // When user did not explicitly request a ref to filter,
                // start with a list of all existing refs
                let mut list =
                    josh_core::housekeeping::list_refs(transaction.repo(), &meta.config.repo)?;

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

        let t2 = service.open_overlay(None)?;
        t2.repo()
            .odb()?
            .add_disk_alternate(repo_path.join("mirror").join("objects").to_str().unwrap())?;
        let (updated_refs, _) = josh_core::filter_refs(
            &t2,
            filter,
            &refs_list,
            josh_core::filter::Filter::new().empty(),
        );
        let mut updated_refs = josh_proxy::refs_locking(updated_refs, &meta);
        josh_core::housekeeping::namespace_refs(&mut updated_refs, temp_ns.name());
        josh_core::update_refs(&t2, &mut updated_refs, &temp_ns.reference(&head_ref));
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

async fn query_meta_repo(
    serv: Arc<JoshProxyService>,
    meta_repo: &str,
    upstream_protocol: UpstreamProtocol,
    upstream_repo: &str,
    remote_auth: &RemoteAuth,
) -> josh_core::JoshResult<josh_proxy::MetaConfig> {
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

    let transaction = serv.open_mirror(Some(&format!(
        "refs/josh/upstream/{}/",
        &josh_core::to_ns(meta_repo)
    )))?;

    let meta_tree = transaction
        .repo()
        .find_reference(&transaction.refname("HEAD"))?
        .peel_to_tree()?;

    let meta_blob = josh_core::filter::tree::get_blob(
        transaction.repo(),
        &meta_tree,
        &std::path::Path::new(&upstream_repo.trim_start_matches('/')).join("config.yml"),
    );

    if meta_blob.is_empty() {
        return Err(josh_core::josh_error("meta repo entry not found"));
    }

    let mut meta: josh_proxy::MetaConfig = Default::default();

    meta.config = serde_yaml::from_str(&meta_blob)?;

    if meta.config.lock_refs {
        let meta_blob = josh_core::filter::tree::get_blob(
            transaction.repo(),
            &meta_tree,
            &std::path::Path::new(&upstream_repo.trim_start_matches('/')).join("lock.yml"),
        );

        if meta_blob.is_empty() {
            return Err(josh_core::josh_error("locked refs not found"));
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
) -> josh_core::JoshResult<MetaConfig> {
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

    let ls_remote = ["git", "ls-remote", url];
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
) -> josh_core::JoshResult<()> {
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
            HeadRef::Explicit(r) => r,
            HeadRef::Implicit => "HEAD",
        }
    }
}

fn head_ref_or_default(head_ref: &str) -> HeadRef {
    let result = head_ref.trim_start_matches(['@', '^']).to_owned();

    if result.is_empty() {
        HeadRef::Implicit
    } else {
        HeadRef::Explicit(result)
    }
}

fn make_repo_update(
    remote_url: &str,
    serv: Arc<JoshProxyService>,
    filter: josh_core::filter::Filter,
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
        filter_spec: josh_core::filter::spec(filter),
        base_ns: josh_core::to_ns(&meta.config.repo),
        git_ns: ns.name().to_string(),
        git_dir: repo_path.display().to_string(),
        mirror_git_dir: serv.repo_path.join("mirror").display().to_string(),
        context_propagator,
    }
}

async fn handle_serve_namespace(
    State(serv): State<Arc<JoshProxyService>>,
    req: Request<Body>,
) -> impl IntoResponse {
    let error_response = |status: StatusCode| (status, "").into_response();

    if req.method() != axum::http::Method::POST {
        return error_response(StatusCode::METHOD_NOT_ALLOWED);
    }

    match req.headers().get(header::CONTENT_TYPE) {
        Some(value) if value == "application/json" => (),
        _ => return error_response(StatusCode::BAD_REQUEST),
    }

    let body = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => return error_response(StatusCode::IM_A_TEAPOT),
    };

    let params = match serde_json::from_slice::<josh_rpc::calls::ServeNamespace>(&body) {
        Err(error) => {
            return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
        Ok(parsed) => parsed,
    };

    let parsed_url = if let Some(mut parsed_url) = FilteredRepoUrl::from_str(&params.query) {
        if parsed_url.filter_spec.is_empty() {
            parsed_url.filter_spec = ":/".to_string();
        }

        parsed_url
    } else {
        return (StatusCode::BAD_REQUEST, "Unable to parse query").into_response();
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error fetching meta repo: {}", e),
            )
                .into_response();
        }
    };

    if is_repo_blocked(&meta_config.config) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Access to this repo is blocked via JOSH_REPO_BLOCK",
        )
            .into_response();
    }

    let upstream = match serv.upstream.get(UpstreamProtocol::Ssh) {
        Some(upstream) => upstream,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "SSH remote is not configured",
            )
                .into_response();
        }
    };

    let remote_url = upstream + meta_config.config.repo.as_str();
    let head_ref = head_ref_or_default(&parsed_url.headref);

    let resolved_ref = match params.command {
        RequestedCommand::GitReceivePack => None,
        _ => {
            let remote_refs = [head_ref.get()];
            let remote_refs =
                match ssh_list_refs(&remote_url, auth_socket, Some(&remote_refs)).await {
                    Ok(remote_refs) => remote_refs,
                    Err(e) => {
                        return (StatusCode::FORBIDDEN, e.to_string()).into_response();
                    }
                };

            match remote_refs.get(head_ref.get()) {
                Some(resolved_ref) => Some(resolved_ref.clone()),
                None => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not resolve remote ref",
                    )
                        .into_response();
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
            return (StatusCode::FORBIDDEN, "Access to upstream repo denied").into_response();
        }
        Err(FetchError::Other(e)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    }

    trace!(
        "handle_serve_namespace_request: filter_spec: {}",
        parsed_url.filter_spec
    );

    let query_filter = match josh_core::filter::parse(&parsed_url.filter_spec) {
        Ok(filter) => filter,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to parse filter: {}", e),
            )
                .into_response();
        }
    };

    let filter = query_filter.chain(match &serv.filter_prefix {
        Some(filter_prefix) => {
            let filter_prefix = match josh_core::filter::parse(filter_prefix) {
                Ok(filter) => filter,
                Err(e) => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!(
                            "Failed to parse prefix filter passed as command line argument: {}",
                            e
                        ),
                    )
                        .into_response();
                }
            };

            meta_config.config.filter.chain(filter_prefix)
        }
        None => meta_config.config.filter,
    });

    let temp_ns = match prepare_namespace(serv.clone(), &meta_config, filter, &head_ref).await {
        Ok(ns) => ns,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
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
        Ok(_) => (StatusCode::NO_CONTENT, "").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Axum middleware for authentication
async fn auth_middleware(req: Request<Body>, next: Next) -> Result<Response<Body>, StatusCode> {
    let (auth, req_without_auth) = match josh_proxy::auth::strip_auth(req) {
        Ok(result) => result,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Store auth in request extensions
    let mut req = req_without_auth;
    req.extensions_mut().insert(auth);

    Ok(next.run(req).await)
}

async fn call_service(
    State(serv): State<Arc<JoshProxyService>>,
    req: Request<Body>,
) -> Result<impl IntoResponse, ProxyError> {
    // Get auth from request extensions
    let auth = req
        .extensions()
        .get::<josh_proxy::auth::Handle>()
        .cloned()
        .unwrap_or(josh_proxy::auth::Handle { hash: None });

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        percent_encoding::percent_decode_str(&path)
            .decode_utf8_lossy()
            .to_string()
    };

    // Need to have some way of passing the filter (via remote path like what github does?)
    let parsed_url = {
        if let Some(parsed_url) = FilteredRepoUrl::from_str(&path) {
            let mut pu = parsed_url;

            if pu.rest.starts_with(':') {
                let guessed_url = path.trim_end_matches("/info/refs");
                let msg = formatdoc!(
                    r#"
                    Invalid URL: "{0}"

                    Note: repository URLs should end with ".git":

                      {0}.git
                    "#,
                    guessed_url
                );
                return Ok(Response::builder()
                    .status(StatusCode::UNPROCESSABLE_ENTITY)
                    .header(header::CONTENT_TYPE, "text/plain")
                    .body(Body::from(msg))
                    .map_err(|e| ProxyError(josh_error(&e.to_string())))?);
            }

            if pu.filter_spec.is_empty() {
                pu.filter_spec = ":/".to_string();
            }
            pu
        } else {
            return if path == "/" {
                let redirect_path = "/~/ui/".to_string();
                Ok(Response::builder()
                    .status(StatusCode::FOUND)
                    .header(header::LOCATION, redirect_path)
                    .body(Body::from(vec![]))
                    .map_err(|e| ProxyError(josh_error(&e.to_string())))?)
            } else {
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from(vec![]))
                    .map_err(|e| ProxyError(josh_error(&e.to_string())))?)
            };
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

    let upstream = match serv.upstream.get(UpstreamProtocol::Http) {
        Some(upstream) => upstream,
        None => {
            return Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                "HTTP remote is not configured",
            )
                .into_response());
        }
    };

    let filter = {
        let filter = meta
            .config
            .filter
            .chain(josh_core::filter::parse(&parsed_url.filter_spec)?);

        if let Some(filter_prefix) = &serv.filter_prefix {
            josh_core::filter::parse(filter_prefix)?.chain(filter)
        } else {
            filter
        }
    };

    let mut fetch_repos = vec![meta.config.repo.clone()];

    let lazy_refs: Vec<_> = josh_core::filter::lazy_refs(filter)
        .iter()
        .map(|x| x.split_once("@").unwrap())
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect();

    fetch_repos.extend(lazy_refs.iter().map(|(x, _y)| x.clone()));

    let remote_url = upstream.clone() + meta.config.repo.as_str();
    let headref = head_ref_or_default(&parsed_url.headref);

    if parsed_url.pathinfo.starts_with("/info/lfs") {
        return Ok(
            Redirect::temporary(&format!("{}{}", remote_url, parsed_url.pathinfo)).into_response(),
        );
    }

    let http_auth_required = serv.require_auth && parsed_url.pathinfo == "/git-receive-pack";

    for fetch_repo in fetch_repos.iter() {
        let fetch_url = upstream.clone() + fetch_repo.as_str();
        if is_repo_blocked(&meta.config) {
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                "Access to this repo is blocked via JOSH_REPO_BLOCK",
            )
                .into_response());
        }

        if !josh_proxy::auth::check_http_auth(&fetch_url, &auth, http_auth_required).await? {
            tracing::trace!("require-auth");

            return Ok(Response::builder()
                .header(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::empty())
                .expect("Failed to build response"));
        }
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
                return Ok(Response::builder()
                    .header(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")
                    .status(StatusCode::UNAUTHORIZED)
                    .body(Body::default())
                    .expect("Failed to build response"));
            }
            Err(FetchError::Other(e)) => {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response());
            }
        }
    }

    if let (Some(q), true) = (
        req.uri().query().map(|x| x.to_string()),
        parsed_url.pathinfo.is_empty(),
    ) {
        let response =
            serve_render_template(serv, q, meta.config.repo, filter, headref.get()).await?;
        return Ok(response);
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

        // Convert hyper response to axum response
        let (parts, body) = response.into_parts();
        let axum_response = Response::from_parts(parts, Body::new(body));

        Ok::<_, JoshError>(axum_response)
    }
    .instrument(tracing::span!(
        tracing::Level::INFO,
        "hyper_cgi / git-http-backend"
    ))
    .await?;

    // This is chained as a separate future to make sure that
    // it is executed in all cases.
    std::mem::drop(temp_ns);

    Ok(cgi_response)
}

async fn serve_render_template(
    serv: Arc<JoshProxyService>,
    q: String,
    upstream_repo: String,
    filter: josh_core::filter::Filter,
    head_ref: &str,
) -> JoshResult<axum::response::Response> {
    let tracing_span = tracing::span!(tracing::Level::TRACE, "serve_render_template");
    let head_ref = head_ref.to_string();
    let res = tokio::task::spawn_blocking(move || -> JoshResult<_> {
        let _span_guard = tracing_span.enter();

        let transaction_mirror = serv.open_mirror(Some(&format!(
            "refs/josh/upstream/{}/",
            &josh_core::to_ns(&upstream_repo)
        )))?;

        let transaction = serv.open_overlay(None)?;

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
        let commit_id = josh_core::filter_commit(
            &transaction,
            filter,
            commit_id,
            josh_core::filter::Filter::new().empty(),
        )?;

        josh_templates::render(&transaction, serv.cache.clone(), "", commit_id, &q, true)
    })
    .in_current_span()
    .await?;

    Ok(match res {
        Ok(Some((res, params))) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                params
                    .get("content-type")
                    .unwrap_or(&"text/plain".to_string()),
            )
            .body(Body::from(res))?,

        Ok(None) => (StatusCode::NOT_FOUND, "File not found").into_response(),

        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
    })
}

async fn prepare_namespace(
    serv: Arc<JoshProxyService>,
    meta: &MetaConfig,
    filter: josh_core::filter::Filter,
    head_ref: &HeadRef,
) -> JoshResult<Arc<josh_proxy::TmpGitNamespace>> {
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

async fn run_polling(serv: Arc<JoshProxyService>) -> josh_core::JoshResult<()> {
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

async fn run_housekeeping(local: std::path::PathBuf, gc: bool) -> josh_core::JoshResult<()> {
    let mut i: usize = 0;
    let cache = std::sync::Arc::new(CacheStack::default());

    loop {
        let local = local.clone();
        let cache = cache.clone();

        tokio::task::spawn_blocking(move || {
            let do_gc = (i % 60 == 0) && gc;
            josh_proxy::housekeeping::run(&local, cache, do_gc)
        })
        .await??;
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        i += 1;
    }
}

fn repo_update_from_env() -> josh_core::JoshResult<josh_proxy::RepoUpdate> {
    let repo_update =
        std::env::var("JOSH_REPO_UPDATE").map_err(|_| josh_error("JOSH_REPO_UPDATE not set"))?;

    serde_json::from_str(&repo_update)
        .map_err(|e| josh_error(&format!("Failed to parse JOSH_REPO_UPDATE: {}", e)))
}

fn pre_receive_hook() -> josh_core::JoshResult<i32> {
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

fn update_hook(refname: &str, old: &str, new: &str) -> josh_core::JoshResult<i32> {
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

async fn handle_graphql(
    State(serv): State<Arc<JoshProxyService>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    // TODO: handle method routing and extraction via axum, instead of raw query/body here
    method: axum::http::Method,
    content_type: Option<axum_extra::extract::TypedHeader<axum_extra::headers::ContentType>>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    auth: Option<axum::extract::Extension<josh_proxy::auth::Handle>>,
    body: String,
) -> Result<impl IntoResponse, ProxyError> {
    // Get auth from request extensions
    let auth = auth
        .map(|auth| auth.0)
        .unwrap_or(josh_proxy::auth::Handle { hash: None });

    // Extract upstream_repo from path (path is everything after /~/graphql)
    let upstream_repo = path.trim_start_matches('/');

    let upstream = match serv.upstream.get(UpstreamProtocol::Http) {
        Some(upstream) => upstream,
        None => {
            return Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                "HTTP remote is not configured",
            )
                .into_response());
        }
    };

    let remote_url = format!("{}/{}", upstream, upstream_repo);
    let content_type = content_type.map(|ct| ct.0.into());

    // Check authentication
    if !josh_proxy::auth::check_http_auth(&remote_url, &auth, serv.require_auth).await? {
        return Ok(Response::builder()
            .header(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::default())
            .expect("Failed to build response"));
    }

    let parsed = match josh_proxy::graphql::parse_req(method, content_type, query, body).await {
        Ok(r) => r,
        Err(resp) => return Ok(resp),
    };

    let transaction_mirror = serv.open_mirror(Some(&format!(
        "refs/josh/upstream/{}/",
        &josh_core::to_ns(&upstream_repo),
    )))?;

    let transaction = serv.open_overlay(None)?;

    transaction.repo().odb()?.add_disk_alternate(
        serv.repo_path
            .join("mirror")
            .join("objects")
            .to_str()
            .unwrap(),
    )?;

    let context = Arc::new(graphql::context(transaction, transaction_mirror));
    let root_node = Arc::new(graphql::repo_schema(
        format!(
            "/{}",
            upstream_repo.strip_suffix(".git").unwrap_or(&upstream_repo)
        ),
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
                Some("HEAD"),
                None,
                false,
            )
            .in_current_span()
            .await
            {
                Ok(_) => {}
                Err(FetchError::AuthRequired) => {
                    return Ok(Response::builder()
                        .header(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::default())
                        .expect("Failed to build response"));
                }
                Err(FetchError::Other(e)) => {
                    return Err(e.into());
                }
            };

            parsed.execute(&root_node, &context).await
        }
    };

    let code = if res.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };

    let response = (code, ErasedJson::pretty(&res)).into_response();

    tokio::task::spawn_blocking(move || -> JoshResult<_> {
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

async fn handle_graphiql(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Result<impl IntoResponse, ProxyError> {
    let upstream_repo = path.trim_start_matches('/');
    let addr = format!("/~/graphql/{}", upstream_repo);

    let response = tokio::task::spawn_blocking(move || josh_proxy::graphql::graphiql(&addr, None))
        .await
        .map_err(|e| ProxyError(josh_error(&e.to_string())))?;

    Ok(response.into_response())
}

async fn shutdown_signal(shutdown_tx: broadcast::Sender<()>) {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    let _ = shutdown_tx.send(());
    println!("shutdown_signal");
}

fn init_trace() -> Option<opentelemetry_sdk::trace::SdkTracerProvider> {
    use opentelemetry::{KeyValue, global, trace::TracerProvider};
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
    use tracing_subscriber::Layer;

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

    if let Ok(endpoint) =
        std::env::var("JOSH_OTLP_ENDPOINT").or(std::env::var("JOSH_JAEGER_ENDPOINT"))
    {
        let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .expect("failed to build OTLP endpoint");

        let resource = opentelemetry_sdk::Resource::builder()
            .with_attribute(KeyValue::new(SERVICE_NAME, service_name.clone()))
            .build();

        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(otlp_exporter)
            .build();

        let tracer = tracer_provider.tracer(service_name);
        global::set_tracer_provider(tracer_provider.clone());

        let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = filter
            .and_then(fmt_layer)
            .and_then(telemetry_layer)
            .with_subscriber(tracing_subscriber::Registry::default());

        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");

        Some(tracer_provider)
    } else {
        let subscriber = filter
            .and_then(fmt_layer)
            .with_subscriber(tracing_subscriber::Registry::default());
        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");

        None
    }
}

async fn run_proxy(args: josh_proxy::cli::Args) -> josh_core::JoshResult<i32> {
    let upstream = make_upstream(&args.remote).inspect_err(|e| {
        eprintln!("Upstream parsing error: {}", e);
    })?;

    let local = std::path::PathBuf::from(&args.local.as_ref().unwrap());
    let local = if local.is_absolute() {
        local
    } else {
        std::env::current_dir()?.join(local)
    };

    josh_proxy::create_repo(&local)?;
    josh_core::cache::sled_load(&local)?;

    let cache = Arc::new(CacheStack::default());

    let proxy_service = Arc::new(JoshProxyService {
        port: args.port.to_string(),
        repo_path: local.to_owned(),
        upstream,
        require_auth: args.require_auth,
        poll_user: args.poll_user,
        cache_duration: args.cache_duration,
        filter_prefix: args.filter_prefix,
        cache,
        fetch_timers: Default::default(),
        heads_map: Default::default(),
        poll: Default::default(),
        fetch_permits: Default::default(),
        filter_permits: Arc::new(tokio::sync::Semaphore::new(10)),
    });

    let ps = proxy_service.clone();

    // Serve static UI files with fallback to index.html only for specific SPA routes
    let ui_router = {
        use tower_http::services::{ServeDir, ServeFile};

        let serve_index = ServeFile::new("/josh/static/index.html");
        Router::new()
            .route_service("/", serve_index.clone())
            .route_service("/select", serve_index.clone())
            .route_service("/browse", serve_index.clone())
            .route_service("/view", serve_index.clone())
            .route_service("/diff", serve_index.clone())
            .route_service("/change", serve_index.clone())
            .route_service("/history", serve_index.clone())
            .fallback_service(ServeDir::new("/josh/static"))
    };

    // Create axum router
    let app = Router::new()
        .route("/version", get(handle_version))
        .route("/remote", get(handle_remote))
        .route("/flush", get(handle_flush))
        .route(
            "/filters",
            get(|State(service): State<Arc<JoshProxyService>>| async move {
                handle_filters(service, false).await
            }),
        )
        .route(
            "/filters/refresh",
            get(|State(service): State<Arc<JoshProxyService>>| async move {
                handle_filters(service, true).await
            }),
        )
        .route("/repo_update", post(handle_repo_update))
        .route("/serve_namespace", post(handle_serve_namespace))
        .nest("/~/ui", ui_router)
        // Serve graphql APIs
        .route(
            "/~/graphql/{*path}",
            post(handle_graphql).get(handle_graphql),
        )
        .route("/~/graphiql/{*path}", get(handle_graphiql))
        .fallback(call_service)
        .layer(middleware::from_fn(auth_middleware))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(josh_proxy::trace::SpanMaker {})
                .on_response(josh_proxy::trace::TraceResponse {})
                .on_request(())
                .on_failure(()),
        )
        .with_state(proxy_service.clone());

    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    let addr: SocketAddr = format!("[::]:{}", args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    let server_future = async move {
        axum::serve(listener, app)
            .await
            .map_err(|e| josh_error(&format!("Server error: {}", e)))
    };

    eprintln!("Now listening on {}", addr);

    if args.no_background {
        tokio::select!(
            r = server_future => eprintln!("http server exited: {:?}", r),
            _ = shutdown_signal(shutdown_tx) => eprintln!("shutdown requested"),
        );
    } else {
        tokio::select!(
            r = run_housekeeping(local, args.gc) => eprintln!("run_housekeeping exited: {:?}", r),
            r = run_polling(ps.clone()) => eprintln!("run_polling exited: {:?}", r),
            r = server_future => eprintln!("http server exited: {:?}", r),
            _ = shutdown_signal(shutdown_tx) => eprintln!("shutdown requested"),
        );
    }

    Ok(0)
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

    let args = josh_proxy::cli::Args::parse();
    let exit_code = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let tracer_provider = init_trace();
            let exit_code = run_proxy(args).await.unwrap_or(1);

            if let Some(tracer_provider) = tracer_provider {
                tracer_provider
                    .shutdown()
                    .expect("failed to shutdown tracer");
            }

            exit_code
        });

    std::process::exit(exit_code);
}
