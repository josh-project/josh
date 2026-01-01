use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::http::ProxyError;
use crate::upstream::{RemoteAuth, RepoUpdate, process_repo_update};
use crate::{FetchError, FilteredRepoUrl, MetaConfig, RepoConfig, cli, run_git_with_auth};

use josh_core::cache::{CacheStack, TransactionContext};
use josh_core::{JoshError, JoshResult, josh_error};
use josh_graphql::graphql;
use josh_rpc::calls::RequestedCommand;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode, header};
use axum::middleware::Next;
use axum::response::IntoResponse;
use serde::Serialize;
use tracing::{Span, trace};
use tracing_futures::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub type FetchTimers = HashMap<String, std::time::Instant>;
pub type Polls = Arc<std::sync::Mutex<HashSet<(String, crate::auth::Handle, String)>>>;

pub type HeadsMap = Arc<RwLock<HashMap<String, String>>>;

#[derive(Serialize, Clone, Debug)]
pub enum JoshProxyUpstream {
    Http(String),
    Ssh(String),
    Both { http: String, ssh: String },
}

impl JoshProxyUpstream {
    pub fn get(&self, protocol: UpstreamProtocol) -> Option<String> {
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UpstreamProtocol {
    Http,
    Ssh,
}

#[derive(Clone)]
pub struct JoshProxyService {
    pub port: String,
    pub repo_path: std::path::PathBuf,
    pub upstream: JoshProxyUpstream,
    pub require_auth: bool,
    pub poll_user: Option<String>,
    pub cache_duration: u64,
    pub filter_prefix: Option<String>,
    pub cache: Arc<CacheStack>,
    pub fetch_timers: Arc<RwLock<FetchTimers>>,
    pub heads_map: HeadsMap,
    pub fetch_permits: Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    pub filter_permits: Arc<tokio::sync::Semaphore>,
    pub poll: Polls,
}

impl JoshProxyService {
    pub fn open_overlay(
        &self,
        ref_prefix: Option<&str>,
    ) -> JoshResult<josh_core::cache::Transaction> {
        TransactionContext::new(self.repo_path.join("overlay"), self.cache.clone()).open(ref_prefix)
    }

    pub fn open_mirror(
        &self,
        ref_prefix: Option<&str>,
    ) -> JoshResult<josh_core::cache::Transaction> {
        TransactionContext::new(self.repo_path.join("mirror"), self.cache.clone()).open(ref_prefix)
    }
}

impl std::fmt::Debug for JoshProxyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("upstream", &self.upstream)
            .finish()
    }
}

/// Turn a list of [cli::Remote] into a [JoshProxyUpstream] struct.
pub fn make_upstream(remotes: &[cli::Remote]) -> JoshResult<JoshProxyUpstream> {
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

fn create_repo_base(path: &PathBuf) -> JoshResult<josh_core::shell::Shell> {
    std::fs::create_dir_all(path).expect("can't create_dir_all");

    if gix::open(path).is_err() {
        gix::init_bare(path)?;
    }

    let credential_helper =
        r#"!f() { echo username="${GIT_USER}"; echo password="${GIT_PASSWORD}"; }; f"#;

    let config_options = [
        ("http", &[("receivepack", "true")] as &[(&str, &str)]),
        (
            "user",
            &[("name", "JOSH"), ("email", "josh@josh-project.dev")],
        ),
        (
            "uploadpack",
            &[
                ("allowAnySHA1InWant", "true"),
                ("allowReachableSHA1InWant", "true"),
                ("allowTipSha1InWant", "true"),
            ],
        ),
        ("receive", &[("advertisePushOptions", "true")]),
        ("gc", &[("auto", "0")]),
        ("credential", &[("helper", credential_helper)]),
    ];

    let shell = josh_core::shell::Shell {
        cwd: path.to_path_buf(),
    };

    let config_source = gix::config::Source::Local;
    let config_location = config_source.storage_location(&mut |_| None).unwrap();
    let config_location = path.join(config_location);

    let mut config =
        gix::config::File::from_path_no_includes(config_location.clone(), config_source)
            .map_err(|_| josh_error("unable to open repo config file"))?;

    config_options
        .iter()
        .cloned()
        .try_for_each(|(section, values)| -> JoshResult<()> {
            let mut section = config
                .new_section(section, None)
                .map_err(|_| josh_error("unable to create config section"))?;

            values
                .iter()
                .cloned()
                .try_for_each(|(name, value)| -> JoshResult<()> {
                    use gix::config::parse::section::ValueName;

                    let key = ValueName::try_from(name)
                        .map_err(|_| josh_error("unable to create config section"))?;
                    let value = Some(value.into());

                    section.push(key, value);

                    Ok(())
                })?;

            Ok(())
        })?;

    fs::write(&config_location, config.to_string())?;

    let hooks = path.join("hooks");
    let packed_refs = path.join("packed-refs");

    if hooks.exists() {
        fs::remove_dir_all(hooks)?;
    }

    if packed_refs.exists() {
        fs::remove_file(packed_refs)?;
    }

    // Delete all files ending with ".lock"
    fs::read_dir(path)?
        .filter_map(|entry| match entry {
            Ok(entry) if entry.path().ends_with(".lock") => Some(path),
            _ => None,
        })
        .map(fs::remove_file)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(shell)
}

pub fn create_repo(path: &std::path::Path) -> JoshResult<()> {
    let mirror_path = path.join("mirror");
    tracing::debug!("init mirror repo: {:?}", mirror_path);
    create_repo_base(&mirror_path)?;

    let overlay_path = path.join("overlay");
    tracing::debug!("init overlay repo: {:?}", overlay_path);
    let overlay_shell = create_repo_base(&overlay_path)?;
    overlay_shell.command(&["mkdir", "hooks"]);

    let josh_executable = std::env::current_exe().expect("can't find path to exe");
    std::os::unix::fs::symlink(
        josh_executable.clone(),
        overlay_path.join("hooks").join("update"),
    )
    .expect("can't symlink update hook");

    std::os::unix::fs::symlink(
        josh_executable,
        overlay_path.join("hooks").join("pre-receive"),
    )
    .expect("can't symlink pre-receive hook");

    if std::env::var_os("JOSH_KEEP_NS").is_none() {
        std::fs::remove_dir_all(overlay_path.join("refs/namespaces")).ok();
    }

    tracing::info!("repo initialized");
    Ok(())
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
    #[allow(clippy::redundant_pattern_matching)]
    if let Err(_) = service.fetch_timers.write().map(|mut t| t.clear()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to clear fetch timers",
        )
            .into_response();
    }

    let body_str = match tokio::task::spawn_blocking(move || -> JoshResult<_> {
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
    repo_path: PathBuf,
    service: Arc<JoshProxyService>,
    meta: MetaConfig,
    temp_ns: Arc<crate::TmpGitNamespace>,
    filter: josh_core::filter::Filter,
    head_ref: &HeadRef,
) -> JoshResult<()> {
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
        let (updated_refs, _) = josh_core::filter_refs(&t2, filter, &refs_list);
        let mut updated_refs = crate::refs_locking(updated_refs, &meta);
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
) -> JoshResult<crate::MetaConfig> {
    let upstream = serv
        .upstream
        .get(upstream_protocol)
        .ok_or(josh_error("no remote specified for the requested protocol"))?;

    let remote_url = [upstream.as_str(), meta_repo].join("");
    match crate::upstream::fetch_upstream(
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

    let mut meta: crate::MetaConfig = crate::MetaConfig {
        config: serde_yaml::from_str(&meta_blob)?,
        ..Default::default()
    };

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
) -> JoshResult<MetaConfig> {
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
                        crate::auth::add_auth(&token)?
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
) -> JoshResult<()> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;

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
                    Some(0) => Ok(()),
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
    ns: Arc<crate::TmpGitNamespace>,
) -> RepoUpdate {
    let context_propagator = crate::trace::make_context_propagator();

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

    match crate::upstream::fetch_upstream(
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
    let (auth, req_without_auth) = match crate::auth::strip_auth(req) {
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
    use axum::response::Redirect;
    use indoc::formatdoc;
    use tokio::process::Command;

    // Get auth from request extensions
    let auth = req
        .extensions()
        .get::<crate::auth::Handle>()
        .cloned()
        .unwrap_or(crate::auth::Handle { hash: None });

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
                return Response::builder()
                    .status(StatusCode::UNPROCESSABLE_ENTITY)
                    .header(header::CONTENT_TYPE, "text/plain")
                    .body(Body::from(msg))
                    .map_err(|e| ProxyError(josh_error(&e.to_string())));
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

        if !crate::auth::check_http_auth(&fetch_url, &auth, http_auth_required).await? {
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
        match crate::upstream::fetch_upstream(
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

        let (axum_response, stderr) = axum_cgi::do_cgi(req, cmd).await;
        tracing::debug!(stderr = %String::from_utf8_lossy(&stderr), "http-backend exited");

        Ok::<_, JoshError>(axum_response)
    }
    .instrument(tracing::span!(
        tracing::Level::INFO,
        "axum-cgi / git-http-backend"
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
        let commit_id = josh_core::filter_commit(&transaction, filter, commit_id)?;

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
) -> JoshResult<Arc<crate::TmpGitNamespace>> {
    let temp_ns = Arc::new(crate::TmpGitNamespace::new(
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

async fn handle_graphql(
    State(serv): State<Arc<JoshProxyService>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    // TODO: handle method routing and extraction via axum, instead of raw query/body here
    method: axum::http::Method,
    content_type: Option<axum_extra::extract::TypedHeader<axum_extra::headers::ContentType>>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    auth: Option<axum::extract::Extension<crate::auth::Handle>>,
    body: String,
) -> Result<impl IntoResponse, ProxyError> {
    use axum_extra::response::ErasedJson;

    // Get auth from request extensions
    let auth = auth
        .map(|auth| auth.0)
        .unwrap_or(crate::auth::Handle { hash: None });

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
    if !crate::auth::check_http_auth(&remote_url, &auth, serv.require_auth).await? {
        return Ok(Response::builder()
            .header(header::WWW_AUTHENTICATE, "Basic realm=User Visible Realm")
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::default())
            .expect("Failed to build response"));
    }

    let parsed = match crate::graphql::parse_req(method, content_type, query, body).await {
        Ok(r) => r,
        Err(resp) => return Ok(resp),
    };

    let transaction_mirror = serv.open_mirror(Some(&format!(
        "refs/josh/upstream/{}/",
        &josh_core::to_ns(upstream_repo),
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
            upstream_repo.strip_suffix(".git").unwrap_or(upstream_repo)
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
            match crate::upstream::fetch_upstream(
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
        let temp_ns = Arc::new(crate::TmpGitNamespace::new(
            &serv.repo_path.join("overlay"),
            tracing::Span::current(),
        ));

        let transaction = &*context.transaction.lock()?;
        let mut to_push = context.to_push.lock()?.clone();

        if let Some((refname, oid)) = crate::merge_meta(
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

            crate::upstream::push_head_url(
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

    let response = tokio::task::spawn_blocking(move || crate::graphql::graphiql(&addr, None))
        .await
        .map_err(|e| ProxyError(josh_error(&e.to_string())))?;

    Ok(response.into_response())
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

        process_repo_update(repo_update)
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

pub fn make_service_router(proxy_service: Arc<JoshProxyService>) -> Router {
    use axum::middleware;
    use axum::routing::{get, post};

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

    Router::new()
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
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(crate::trace::SpanMaker {})
                .on_response(crate::trace::TraceResponse {})
                .on_request(())
                .on_failure(()),
        )
        .with_state(proxy_service.clone())
}
