pub mod auth;
pub mod cli;
pub mod graphql;
pub mod housekeeping;
pub mod http;
pub mod serve;
pub mod service;
pub mod trace;
pub mod upstream;

use josh_core::{JoshError, josh_error};

use crate::upstream::RemoteAuth;

josh_core::regex_parsed!(
    FilteredRepoUrl,
    r"(?P<api>/~/\w+)?(?P<upstream_repo>/[^:!]*[.]git)(?P<headref>[\^@][^:!]*)?((?P<filter_spec>[:!].*)[.]git)?(?P<pathinfo>/.*)?(?P<rest>.*)",
    [api, upstream_repo, filter_spec, pathinfo, headref, rest]
);

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Ref {
    pub target: josh_core::Oid,
}

fn make_ssh_command() -> String {
    let ssh_options = [
        "LogLevel=ERROR",
        "UserKnownHostsFile=/dev/null",
        "StrictHostKeyChecking=no",
        "PreferredAuthentications=publickey",
        "ForwardX11=no",
        "ForwardAgent=no",
    ];

    let ssh_options = ssh_options.map(|o| format!("-o {}", o));
    format!("ssh {}", ssh_options.join(" "))
}

pub fn run_git_with_auth(
    cwd: &std::path::Path,
    cmd: &[&str],
    remote_auth: &RemoteAuth,
    alt_object_dir: Option<String>,
) -> josh_core::JoshResult<(String, String, i32)> {
    let shell = josh_core::shell::Shell {
        cwd: cwd.to_owned(),
    };

    let maybe_object_dir = match &alt_object_dir {
        Some(dir) => {
            vec![("GIT_ALTERNATE_OBJECT_DIRECTORIES", dir.as_str())]
        }
        None => vec![],
    };

    match remote_auth {
        RemoteAuth::Ssh { auth_socket } => {
            let ssh_command = make_ssh_command();
            let auth_socket = auth_socket.clone().into_os_string();
            let auth_socket = auth_socket
                .to_str()
                .ok_or(josh_error("failed to convert path"))?;

            let env = [("GIT_SSH_COMMAND", ssh_command.as_str())];
            let env_notrace = [
                [("SSH_AUTH_SOCK", auth_socket)].as_slice(),
                maybe_object_dir.as_slice(),
            ]
            .concat();

            Ok(shell.command_env(cmd, &env, &env_notrace))
        }
        RemoteAuth::Http { auth } => {
            let (username, password) = auth.parse().unwrap_or_default();
            let env_notrace = [
                [
                    ("GIT_PASSWORD", password.as_str()),
                    ("GIT_USER", username.as_str()),
                ]
                .as_slice(),
                maybe_object_dir.as_slice(),
            ]
            .concat();

            Ok(shell.command_env(cmd, &[], &env_notrace))
        }
    }
}

pub fn get_head(
    path: &std::path::Path,
    url: &str,
    remote_auth: &RemoteAuth,
) -> josh_core::JoshResult<String> {
    let cmd = &["git", "ls-remote", "--symref", url, "HEAD"];

    tracing::info!("get_head {:?} {:?} {:?}", cmd, path, "");
    let (stdout, _, code) = run_git_with_auth(path, cmd, remote_auth, None)?;

    if code != 0 {
        return Err(josh_error(&format!(
            "git subprocess exited with code {}",
            code
        )));
    }

    let head = stdout
        .lines()
        .next()
        .unwrap_or("refs/heads/master")
        .to_string();

    let head = head.replacen("ref: ", "", 1);
    let head = head.replacen("\tHEAD", "", 1);

    Ok(head)
}

pub enum FetchError {
    AuthRequired,
    Other(JoshError),
}

impl<T> From<T> for FetchError
where
    T: std::error::Error,
{
    fn from(e: T) -> Self {
        FetchError::Other(JoshError::from(e))
    }
}

impl FetchError {
    pub fn from_josh_error(e: JoshError) -> Self {
        FetchError::Other(e)
    }
}

pub fn fetch_refs_from_url(
    path: &std::path::Path,
    upstream_repo: &str,
    url: &str,
    refs_prefixes: &[String],
    remote_auth: &RemoteAuth,
) -> Result<(), FetchError> {
    let specs: Vec<_> = refs_prefixes
        .iter()
        .map(|r| {
            format!(
                "+{}:refs/josh/upstream/{}/{}",
                &r,
                josh_core::to_ns(upstream_repo),
                &r
            )
        })
        .collect();

    let cmd = ["git", "fetch", "--prune", "--no-tags", url]
        .map(str::to_owned)
        .to_vec();
    let cmd = cmd.into_iter().chain(specs).collect::<Vec<_>>();
    let cmd = cmd.iter().map(|s| s as &str).collect::<Vec<&str>>();

    tracing::info!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

    let (_, stderr, code) =
        run_git_with_auth(path, &cmd, remote_auth, None).map_err(FetchError::Other)?;

    tracing::debug!("fetch_refs_from_url done {:?} {:?} {:?}", cmd, path, stderr);

    if stderr.contains("fatal: Authentication failed")
        || stderr.contains("fatal: Could not read")
        || stderr.contains(": Permission denied")
    {
        return Err(FetchError::AuthRequired);
    }

    if stderr.contains("fatal:") || code != 0 {
        tracing::error!("{:?}", stderr);
        return Err(FetchError::Other(josh_error(&format!(
            "git process exited with code {}: {:?}",
            code, stderr
        ))));
    }

    if stderr.contains("error:") {
        tracing::error!("{:?}", stderr);
        return Err(FetchError::Other(josh_core::josh_error(&format!(
            "git error: {:?}",
            stderr
        ))));
    }

    Ok(())
}

type IoCleanupSender = tokio::sync::mpsc::UnboundedSender<service::IoCleanup>;

pub struct TmpGitNamespace {
    name: String,
    repo_path: std::path::PathBuf,
    cleanup: Option<IoCleanupSender>,
    _span: tracing::Span,
}

impl TmpGitNamespace {
    pub fn new(
        repo_path: &std::path::Path,
        span: tracing::Span,
        cleanup: Option<IoCleanupSender>,
    ) -> TmpGitNamespace {
        let n = format!("request_{}", uuid::Uuid::new_v4());
        let n2 = n.clone();
        TmpGitNamespace {
            name: n,
            repo_path: repo_path.to_owned(),
            cleanup,
            _span: tracing::span!(
                parent: span,
                tracing::Level::TRACE,
                "TmpGitNamespace",
                name = n2.as_str(),
            ),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn reference(&self, refname: &str) -> String {
        format!("refs/namespaces/{}/{}", &self.name, refname)
    }

    pub fn cleanup(repo_path: &std::path::Path, name: &str) {
        let ns_path = repo_path.join("refs/namespaces").join(&name);

        if !std::path::Path::new(&ns_path).exists() {
            return;
        }

        if let Err(e) = std::fs::remove_dir_all(&ns_path) {
            tracing::error!(path = %ns_path.display(), error = %e, "remove_dir_all failed",)
        }
    }
}

impl std::fmt::Debug for TmpGitNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("name", &self.name)
            .finish()
    }
}

impl Drop for TmpGitNamespace {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            if let Err(e) = cleanup.send(service::IoCleanup {
                repo_path: self.repo_path.clone(),
                name: self.name.clone(),
            }) {
                tracing::error!(error = %e, "failed to schedule deferred cleanup")
            }
        } else {
            tracing::warn!("Cleaning up namespace synchronously");
            Self::cleanup(&self.repo_path, &self.name);
        }
    }
}

fn proxy_commit_signature<'a>() -> josh_core::JoshResult<git2::Signature<'a>> {
    Ok(if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        git2::Signature::new(
            "JOSH",
            "josh@josh-project.dev",
            &git2::Time::new(time.parse()?, 0),
        )?
    } else {
        git2::Signature::now("JOSH", "josh@josh-project.dev")?
    })
}

pub fn merge_meta(
    transaction: &josh_core::cache::Transaction,
    transaction_mirror: &josh_core::cache::Transaction,
    meta_add: &std::collections::HashMap<std::path::PathBuf, Vec<String>>,
) -> josh_core::JoshResult<Option<(String, git2::Oid)>> {
    if meta_add.is_empty() {
        return Ok(None);
    }
    let rev = transaction_mirror.refname("refs/josh/meta");

    let r = transaction_mirror.repo().revparse_single(&rev);
    let (tree, parent) = if let Ok(r) = r {
        let meta_commit = transaction.repo().find_commit(r.id())?;
        let tree = meta_commit.tree()?;
        (tree, Some(meta_commit))
    } else {
        (josh_core::filter::tree::empty(transaction.repo()), None)
    };

    let mut tree = tree;

    for (path, add_lines) in meta_add.iter() {
        let prev = if let Ok(e) = tree.get_path(path) {
            let blob = transaction.repo().find_blob(e.id())?;
            std::str::from_utf8(blob.content())?.to_owned()
        } else {
            "".to_owned()
        };

        let mut lines = prev
            .split('\n')
            .filter(|x| !(*x).is_empty())
            .collect::<Vec<_>>();
        for marker in add_lines {
            lines.push(marker);
        }
        lines.sort_unstable();
        lines.dedup();

        let blob = transaction.repo().blob(lines.join("\n").as_bytes())?;

        tree = josh_core::filter::tree::insert(transaction.repo(), &tree, path, blob, 0o0100644)?;
    }

    let signature = proxy_commit_signature()?;
    let oid = transaction.repo().commit(
        None,
        &signature,
        &signature,
        "marker",
        &tree,
        &parent.as_ref().into_iter().collect::<Vec<_>>(),
    )?;

    Ok(Some(("refs/josh/meta".to_string(), oid)))
}
