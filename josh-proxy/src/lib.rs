pub mod auth;
pub mod cli;
pub mod juniper_hyper;
pub mod trace;

#[macro_use]
extern crate lazy_static;

use josh::history::RewriteData;
use josh::{JoshError, JoshResult, josh_error};
use std::fs;
use std::path::PathBuf;

#[derive(PartialEq)]
enum PushMode {
    Normal,
    Review,
    Stack,
    Split,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Ref {
    pub target: josh::Oid,
}

type RefsLock = std::collections::HashMap<String, josh::Oid>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct RepoConfig {
    pub repo: String,

    #[serde(default)]
    pub filter: josh::filter::Filter,

    #[serde(default)]
    pub lock_refs: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MetaConfig {
    pub config: RepoConfig,
    pub refs_lock: RefsLock,
}

pub fn refs_locking(refs: Vec<(String, git2::Oid)>, meta: &MetaConfig) -> Vec<(String, git2::Oid)> {
    if !meta.config.lock_refs {
        return refs;
    }
    let mut output = vec![];

    for (n, _) in refs.into_iter() {
        if let Some(lid) = meta.refs_lock.get(&n) {
            output.push((n, (*lid).into()));
        }
    }

    output
}

fn baseref_and_options(refname: &str) -> josh::JoshResult<(String, String, Vec<String>, PushMode)> {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().ok_or(josh::josh_error("no next"))?.to_owned();

    let options = if let Some(options) = split.next() {
        options.split(',').map(|x| x.to_string()).collect()
    } else {
        vec![]
    };

    let mut baseref = push_to.to_owned();
    let mut push_mode = PushMode::Normal;

    if baseref.starts_with("refs/for") {
        push_mode = PushMode::Review;
        baseref = baseref.replacen("refs/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/drafts") {
        push_mode = PushMode::Review;
        baseref = baseref.replacen("refs/drafts", "refs/heads", 1)
    }
    if baseref.starts_with("refs/stack/for") {
        push_mode = PushMode::Stack;
        baseref = baseref.replacen("refs/stack/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/split/for") {
        push_mode = PushMode::Split;
        baseref = baseref.replacen("refs/split/for", "refs/heads", 1)
    }
    Ok((baseref, push_to, options, push_mode))
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum RemoteAuth {
    Http { auth: auth::Handle },
    Ssh { auth_socket: PathBuf },
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct RepoUpdate {
    pub refs: std::collections::HashMap<String, (String, String)>,
    pub remote_url: String,
    pub remote_auth: RemoteAuth,
    pub port: String,
    pub filter_spec: String,
    pub base_ns: String,
    pub git_ns: String,
    pub git_dir: String,
    pub mirror_git_dir: String,
    pub context_propagator: std::collections::HashMap<String, String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
pub struct PushOptions {
    pub merge: bool,
    pub create: bool,
    pub force: bool,
    pub base: Option<String>,
    pub author: Option<String>,
}

impl Default for PushOptions {
    fn default() -> Self {
        PushOptions {
            merge: false,
            create: false,
            force: false,
            base: None,
            author: None,
        }
    }
}

pub fn process_repo_update(repo_update: RepoUpdate) -> josh::JoshResult<String> {
    let push_options_path = std::path::PathBuf::from(&repo_update.git_dir)
        .join("refs/namespaces")
        .join(&repo_update.git_ns)
        .join("push_options");

    let push_options = std::fs::read_to_string(&push_options_path)?;
    std::fs::remove_file(push_options_path).ok();

    let push_options: PushOptions = serde_json::from_str(&push_options)
        .map_err(|e| josh_error(&format!("Failed to parse push options: {}", e)))?;

    tracing::debug!(
        push_options = ?push_options,
        "process_repo_update"
    );

    for (refname, (old, new)) in repo_update.refs.iter() {
        let transaction = josh::cache::Transaction::open(
            std::path::Path::new(&repo_update.git_dir),
            Some(&format!("refs/josh/upstream/{}/", repo_update.base_ns)),
        )?;

        let transaction_mirror = josh::cache::Transaction::open(
            std::path::Path::new(&repo_update.mirror_git_dir),
            Some(&format!("refs/josh/upstream/{}/", repo_update.base_ns)),
        )?;

        transaction.repo().odb()?.add_disk_alternate(
            transaction_mirror
                .repo()
                .path()
                .join("objects")
                .to_str()
                .unwrap(),
        )?;

        let old = git2::Oid::from_str(old)?;
        let (baseref, push_to, options, push_mode) = baseref_and_options(refname)?;

        let old = if old == git2::Oid::zero() {
            let rev = format!("refs/namespaces/{}/{}", repo_update.git_ns, &baseref);
            let oid = if let Ok(resolved) = transaction.repo().revparse_single(&rev) {
                resolved.id()
            } else {
                old
            };

            tracing::debug!(
                old_oid = ?oid,
                rev = %rev,
                "resolve_old"
            );

            oid
        } else {
            tracing::debug!(
                old_oid = ?old,
                refname = %refname,
                "resolve_old"
            );

            old
        };

        let original_target_ref = if let Some(base) = &push_options.base {
            // Allow user to use just the branch name as the base:
            let full_path_base_refname =
                transaction_mirror.refname(&format!("refs/heads/{}", base));
            if transaction_mirror
                .repo()
                .refname_to_id(&full_path_base_refname)
                .is_ok()
            {
                full_path_base_refname
            } else {
                transaction_mirror.refname(&base)
            }
        } else {
            transaction_mirror.refname(&baseref)
        };

        let original_target = if let Ok(oid) = transaction_mirror
            .repo()
            .refname_to_id(&original_target_ref)
        {
            tracing::debug!(
                original_target_oid = ?oid,
                original_target_ref = %original_target_ref,
                "resolve_original_target"
            );

            oid
        } else {
            tracing::debug!(
                original_target_ref = %original_target_ref,
                "resolve_original_target"
            );

            return Err(josh::josh_error(&unindent::unindent(&format!(
                r###"
                    Reference {:?} does not exist on remote.
                    If you want to create it, pass "-o base=<basebranch>" or "-o base=path/to/ref"
                    to specify a base branch/reference.
                    "###,
                baseref
            ))));
        };

        let reparent_orphans = if push_options.create {
            Some(original_target)
        } else {
            None
        };

        let author = if let Some(author) = &push_options.author {
            author.to_string()
        } else {
            "".to_string()
        };

        let mut changes =
            if push_mode == PushMode::Stack || push_mode == PushMode::Split || author != "" {
                Some(vec![])
            } else {
                None
            };

        let filter = josh::filter::parse(&repo_update.filter_spec)?;
        let new_oid = git2::Oid::from_str(new)?;
        let backward_new_oid = {
            let unapply_result = josh::history::unapply_filter(
                &transaction,
                filter,
                original_target,
                old,
                new_oid,
                push_options.merge,
                reparent_orphans,
                &mut changes,
            )?;

            tracing::debug!(
                processed_old = ?old,
                unapply_result = ?unapply_result,
                "unapply_filter"
            );

            unapply_result
        };

        let oid_to_push = if push_options.merge {
            let backward_commit = transaction.repo().find_commit(backward_new_oid)?;
            if let Ok(base_commit_id) = transaction_mirror
                .repo()
                .revparse_single(&original_target_ref)
                .map(|x| x.id())
            {
                let signature = proxy_commit_signature()?;
                let base_commit = transaction.repo().find_commit(base_commit_id)?;
                let merged_tree = transaction
                    .repo()
                    .merge_commits(&base_commit, &backward_commit, None)?
                    .write_tree_to(transaction.repo())?;
                transaction.repo().commit(
                    None,
                    &signature,
                    &signature,
                    &format!("Merge from {}", &repo_update.filter_spec),
                    &transaction.repo().find_tree(merged_tree)?,
                    &[&base_commit, &backward_commit],
                )?
            } else {
                return Err(josh::josh_error("josh_merge failed"));
            }
        } else {
            backward_new_oid
        };

        let ref_with_options = if !options.is_empty() {
            format!("{}{}{}", push_to, "%", options.join(","))
        } else {
            push_to
        };

        let to_push = if let Some(changes) = changes {
            let mut v = vec![];
            v.append(&mut changes_to_refs(&baseref, &author, changes)?);

            if push_mode == PushMode::Split {
                split_changes(transaction.repo(), &mut v, old)?;
            }

            if push_mode == PushMode::Review {
                v.push((ref_with_options, oid_to_push, "JOSH_PUSH".to_string()));
            }

            v.push((
                format!(
                    "refs/heads/@heads/{}/{}",
                    baseref.replacen("refs/heads/", "", 1),
                    author,
                ),
                oid_to_push,
                baseref.replacen("refs/heads/", "", 1),
            ));
            v
        } else {
            vec![(ref_with_options, oid_to_push, "JOSH_PUSH".to_string())]
        };

        let mut resp = vec![];

        for (reference, oid, display_name) in to_push {
            let force_push = push_mode != PushMode::Normal || push_options.force;

            let (text, status) = push_head_url(
                transaction.repo(),
                &format!("{}/objects", repo_update.mirror_git_dir),
                oid,
                &reference,
                &repo_update.remote_url,
                &repo_update.remote_auth,
                &repo_update.git_ns,
                &display_name,
                force_push,
            )?;

            if status != 0 {
                return Err(josh::josh_error(&text));
            }

            resp.push(text.to_string());
            let mut warnings = josh::filter::compute_warnings(
                &transaction,
                filter,
                transaction.repo().find_commit(oid)?.tree()?,
            );

            if !warnings.is_empty() {
                resp.push("warnings:".to_string());
                resp.append(&mut warnings);
            }
        }

        let reapply = josh::filter::apply_to_commit(
            filter,
            &transaction.repo().find_commit(oid_to_push)?,
            &transaction,
        )?;

        if new_oid != reapply {
            if std::env::var("JOSH_REWRITE_REFS").is_ok() {
                transaction.repo().reference(
                    &format!(
                        "refs/josh/rewrites/{}/{:?}/r_{}",
                        repo_update.base_ns,
                        filter.id(),
                        reapply
                    ),
                    reapply,
                    true,
                    "reapply",
                )?;
            }

            tracing::debug!(
                new_oid = ?new_oid,
                reapply = ?reapply,
                "rewrite"
            );

            let text = format!("REWRITE({} -> {})", new_oid, reapply);
            resp.push(text);
        }

        return Ok(resp.join("\n"));
    }

    Ok("".to_string())
}

fn split_changes(
    repo: &git2::Repository,
    changes: &mut Vec<(String, git2::Oid, String)>,
    base: git2::Oid,
) -> josh::JoshResult<()> {
    if base == git2::Oid::zero() {
        return Ok(());
    }

    let commits: Vec<git2::Commit> = changes
        .iter()
        .map(|(_, commit, _)| repo.find_commit(*commit).unwrap())
        .collect();

    let mut trees: Vec<git2::Tree> = commits
        .iter()
        .map(|commit| commit.tree().unwrap())
        .collect();

    trees.insert(0, repo.find_commit(base)?.tree()?);

    let diffs: Vec<git2::Diff> = (1..trees.len())
        .map(|i| {
            repo.diff_tree_to_tree(Some(&trees[i - 1]), Some(&trees[i]), None)
                .unwrap()
        })
        .collect();

    let mut moved = std::collections::HashSet::new();
    let mut bases = vec![base];
    for _ in 0..changes.len() {
        let mut new_bases = vec![];
        for base in bases.iter() {
            for i in 0..diffs.len() {
                if moved.contains(&i) {
                    continue;
                }
                let diff = &diffs[i];
                let parent = repo.find_commit(*base)?;
                if let Ok(mut index) = repo.apply_to_tree(&parent.tree()?, diff, None) {
                    moved.insert(i);
                    let new_tree = repo.find_tree(index.write_tree_to(repo)?)?;
                    let new_commit = josh::history::rewrite_commit(
                        repo,
                        &repo.find_commit(changes[i].1)?,
                        &[&parent],
                        RewriteData {
                            tree: new_tree,
                            author: None,
                            committer: None,
                            message: None,
                        },
                        false,
                    )?;
                    changes[i].1 = new_commit;
                    new_bases.push(new_commit);
                }
                if moved.len() == changes.len() {
                    return Ok(());
                }
            }
        }
        bases = new_bases;
    }

    Ok(())
}

pub fn push_head_url(
    repo: &git2::Repository,
    alternate: &str,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    remote_auth: &RemoteAuth,
    namespace: &str,
    display_name: &str,
    force: bool,
) -> josh::JoshResult<(String, i32)> {
    let push_temp_ref = format!("refs/{}", &namespace);
    let push_refspec = format!("{}:{}", &push_temp_ref, &refname);

    let mut cmd = vec!["git", "push"];
    if force {
        cmd.push("--force")
    }
    cmd.push(url);
    cmd.push(&push_refspec);

    let mut fake_head = repo.reference(&push_temp_ref, oid, true, "push_head_url")?;
    let (stdout, stderr, status) =
        run_git_with_auth(repo.path(), &cmd, remote_auth, Some(alternate.to_owned()))?;
    fake_head.delete()?;

    tracing::debug!(
        stdout = %stdout,
        stderr = %stderr,
        status = %status,
        "push_head_url: run_git"
    );

    let stderr = stderr.replace(&push_temp_ref, display_name);
    Ok((stderr, status))
}

fn create_repo_base(path: &PathBuf) -> josh::JoshResult<josh::shell::Shell> {
    std::fs::create_dir_all(path).expect("can't create_dir_all");

    if !gix::open(path).is_ok() {
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

    let shell = josh::shell::Shell {
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

pub fn create_repo(path: &std::path::Path) -> josh::JoshResult<()> {
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
) -> josh::JoshResult<(String, String, i32)> {
    let shell = josh::shell::Shell {
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
) -> josh::JoshResult<String> {
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
                josh::to_ns(upstream_repo),
                &r
            )
        })
        .collect();

    let cmd = ["git", "fetch", "--prune", "--no-tags", url]
        .map(str::to_owned)
        .to_vec();
    let cmd = cmd.into_iter().chain(specs.into_iter()).collect::<Vec<_>>();
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
        return Err(FetchError::Other(josh::josh_error(&format!(
            "git error: {:?}",
            stderr
        ))));
    }

    Ok(())
}

pub struct TmpGitNamespace {
    name: String,
    repo_path: std::path::PathBuf,
    _span: tracing::Span,
}

impl TmpGitNamespace {
    pub fn new(repo_path: &std::path::Path, span: tracing::Span) -> TmpGitNamespace {
        let n = format!("request_{}", uuid::Uuid::new_v4());
        let n2 = n.clone();
        TmpGitNamespace {
            name: n,
            repo_path: repo_path.to_owned(),
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
        if std::env::var_os("JOSH_KEEP_NS").is_some() {
            return;
        }

        let request_tmp_namespace = self.repo_path.join("refs/namespaces").join(&self.name);

        if std::path::Path::new(&request_tmp_namespace).exists() {
            fs::remove_dir_all(&request_tmp_namespace).unwrap_or_else(|err| {
                tracing::error!(
                    "remove_dir_all {} failed, error: {}",
                    request_tmp_namespace.display(),
                    err
                )
            });
        }
    }
}

fn changes_to_refs(
    baseref: &str,
    change_author: &str,
    changes: Vec<josh::Change>,
) -> josh::JoshResult<Vec<(String, git2::Oid, String)>> {
    let mut seen = vec![];
    let mut changes = changes;
    changes.retain(|change| change.author == change_author);
    if !change_author.contains('@') {
        return Err(josh::josh_error(
            "Push option 'author' needs to be set to a valid email address",
        ));
    };

    for change in changes.iter() {
        if let Some(id) = &change.id {
            if id.contains('@') {
                return Err(josh::josh_error("Change id must not contain '@'"));
            }
            if seen.contains(&id) {
                return Err(josh::josh_error(&format!(
                    "rejecting to push {:?} with duplicate label",
                    change.commit
                )));
            }
            seen.push(id);
        } else {
            return Err(josh::josh_error(&format!(
                "rejecting to push {:?} without id",
                change.commit
            )));
        }
    }

    Ok(changes
        .iter()
        .map(|change| {
            (
                format!(
                    "refs/heads/@changes/{}/{}/{}",
                    baseref.replacen("refs/heads/", "", 1),
                    change.author,
                    change.id.as_ref().unwrap_or(&"".to_string()),
                ),
                change.commit,
                change
                    .id
                    .as_ref()
                    .unwrap_or(&"JOSH_PUSH".to_string())
                    .to_string(),
            )
        })
        .collect())
}

fn proxy_commit_signature<'a>() -> josh::JoshResult<git2::Signature<'a>> {
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
    transaction: &josh::cache::Transaction,
    transaction_mirror: &josh::cache::Transaction,
    meta_add: &std::collections::HashMap<std::path::PathBuf, Vec<String>>,
) -> josh::JoshResult<Option<(String, git2::Oid)>> {
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
        (josh::filter::tree::empty(transaction.repo()), None)
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

        tree = josh::filter::tree::insert(transaction.repo(), &tree, path, blob, 0o0100644)?;
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
