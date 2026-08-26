use anyhow::{Context, anyhow};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::str::FromStr;

/// Resolve the `input_ref` argument to a commit OID.
///
/// - `"+"`: Creates a temporary commit from the current index (staged changes
///   on top of HEAD).
/// - `"."`: Creates a temporary commit from the working tree (all tracked and
///   untracked files under the repo root).
/// - A raw SHA hex string: resolves the object and peels to its commit.
/// - Anything else: treated as a ref name.
pub fn resolve_snapshot_input(
    transaction: &crate::cache::Transaction,
    input_ref: &str,
) -> anyhow::Result<gix_hash::ObjectId> {
    if input_ref == "+" || input_ref == "." {
        let tree = if input_ref == "+" {
            parse_oid(&git_stdout(transaction, &["write-tree"], &[])?)?
        } else {
            let temp = tempfile::tempdir()?;
            let index = temp.path().join("index");
            let index = index
                .to_str()
                .context("temporary index path is not valid UTF-8")?;
            let env = [("GIT_INDEX_FILE", index)];
            git_stdout(transaction, &["read-tree", "HEAD"], &env)?;
            git_stdout(transaction, &["add", "--all"], &env)?;
            parse_oid(&git_stdout(transaction, &["write-tree"], &env)?)?
        };
        let head = transaction.head().context("could not resolve HEAD")?.commit;
        let signature = josh_actor_signature()?;
        crate::objects::write_commit(
            transaction.odb(),
            tree,
            &[head],
            &signature,
            &signature,
            "WIP",
        )
    } else {
        let oid = transaction
            .rev_parse(input_ref)?
            .with_context(|| format!("could not resolve input: {input_ref:?}"))?;
        crate::objects::peel_to_commit(transaction.odb(), oid)
            .with_context(|| format!("could not peel input to a commit: {input_ref:?}"))
    }
}

const JOSH_COMMIT_TIME_ENV: &str = "JOSH_COMMIT_TIME";
const JOSH_COMMIT_NAME: &str = "JOSH";
const JOSH_COMMIT_EMAIL: &str = "josh@josh-project.dev";

fn parse_oid(bytes: &[u8]) -> anyhow::Result<gix_hash::ObjectId> {
    gix_hash::ObjectId::from_str(
        std::str::from_utf8(bytes)
            .context("git returned a non-UTF-8 object ID")?
            .trim(),
    )
    .context("git returned an invalid object ID")
}

/// Josh's fixed commit identity for commits written through the object store.
pub fn josh_actor_signature() -> anyhow::Result<gix_actor::Signature> {
    let time = match std::env::var(JOSH_COMMIT_TIME_ENV) {
        Ok(time) => gix_actor::date::Time {
            seconds: time.parse()?,
            offset: 0,
        },
        Err(_) => gix_actor::date::Time::now_local_or_utc(),
    };
    Ok(gix_actor::Signature {
        name: JOSH_COMMIT_NAME.into(),
        email: JOSH_COMMIT_EMAIL.into(),
        time,
    })
}

/// Parse a date string from `GIT_COMMITTER_DATE` / `GIT_AUTHOR_DATE`. Accepts the
/// formats git typically uses: raw (`<unix> <offset>`), RFC 2822 (what `date -R`
/// emits) and RFC 3339 / ISO 8601.
fn parse_git_env_date(s: &str) -> Option<gix_actor::date::Time> {
    let s = s.trim();
    if let Some((secs, offset)) = s.split_once(' ') {
        if let (Ok(seconds), Ok(offset)) = (secs.parse::<i64>(), offset.parse::<i32>()) {
            let offset_minutes = (offset / 100) * 60 + (offset % 100);
            return Some(gix_actor::date::Time {
                seconds,
                offset: offset_minutes * 60,
            });
        }
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(gix_actor::date::Time {
            seconds: dt.timestamp(),
            offset: dt.offset().local_minus_utc(),
        });
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(gix_actor::date::Time {
            seconds: dt.timestamp(),
            offset: dt.offset().local_minus_utc(),
        });
    }
    None
}

/// Like [`crate::cache::Transaction::signature`] but honors
/// `GIT_COMMITTER_*` / `GIT_AUTHOR_*` environment variables, including the date.
pub fn user_signature(
    transaction: &crate::cache::Transaction,
) -> anyhow::Result<gix_actor::Signature> {
    let default = transaction.signature()?;
    let name = std::env::var("GIT_COMMITTER_NAME")
        .or_else(|_| std::env::var("GIT_AUTHOR_NAME"))
        .map(Into::into)
        .unwrap_or(default.name);
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .or_else(|_| std::env::var("GIT_AUTHOR_EMAIL"))
        .map(Into::into)
        .unwrap_or(default.email);
    let time = std::env::var("GIT_COMMITTER_DATE")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok())
        .as_deref()
        .and_then(parse_git_env_date)
        .unwrap_or(default.time);

    Ok(gix_actor::Signature { name, email, time })
}

/// Resolve a repository path to its working directory.
///
/// Callers typically pass a gitdir (e.g. `repo.path()`) and want the working
/// tree to use as a cwd for git commands. Opening the repository yields the
/// correct working directory even for linked worktrees, where the gitdir is
/// `<main>/.git/worktrees/<name>` and naively stripping a trailing `.git`
/// would not produce the worktree. The function is idempotent on an
/// already-normalized working directory.
///
/// Falls back to stripping a trailing `.git` component when the path cannot be
/// opened as a repository or the repository is bare (no working tree).
pub fn normalize_repo_path(repo_path: &std::path::Path) -> PathBuf {
    if let Ok(repo) = gix::open(repo_path)
        && let Some(workdir) = repo.workdir()
    {
        let workdir = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.into());
        let mut workdir = workdir.into_os_string();
        workdir.push(std::path::MAIN_SEPARATOR_STR);
        return workdir.into();
    }

    let components = repo_path.components().collect::<Vec<_>>();

    if let Some((last, components)) = components.split_last()
        && last == &std::path::Component::Normal(".git".as_ref())
    {
        components.iter().collect()
    } else {
        repo_path.into()
    }
}

pub(crate) fn map_discovery_error(error: gix::discover::Error) -> anyhow::Error {
    // Preserve the long-standing CLI error for "not in a repository"; gix's discovery error
    // embeds the absolute current directory, which makes the message unstable.
    if error
        .to_string()
        .starts_with("Could not find a git repository")
    {
        anyhow!("could not find repository at '.'; class=Repository (6); code=NotFound (-3)")
    } else {
        anyhow::Error::new(error)
    }
}

/// Repository paths discovered with Git's environment overrides.
pub struct RepositoryPaths {
    pub workdir_or_gitdir: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

pub fn discover_repository_paths() -> anyhow::Result<RepositoryPaths> {
    let repo = gix::discover_with_environment_overrides(std::env::current_dir()?)
        .map_err(map_discovery_error)?;
    Ok(RepositoryPaths {
        workdir_or_gitdir: repo.workdir().unwrap_or_else(|| repo.path()).to_owned(),
        git_dir: repo.path().to_owned(),
        common_dir: repo.common_dir().to_owned(),
    })
}

/// A per-file line-count summary for a commit diff.
pub struct FileStat {
    pub path: String,
    pub adds: usize,
    pub dels: usize,
}

/// Compare `commit_oid` with its first parent and count changed lines per path.
pub fn file_stats(
    transaction: &crate::cache::Transaction,
    commit_oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<FileStat>> {
    let oid = commit_oid.to_string();
    let output = git_stdout(
        transaction,
        &[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--numstat",
            "-r",
            "--no-renames",
            "-z",
            &oid,
        ],
        &[],
    )?;
    let mut files = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let adds = parse_numstat(fields.next())?;
        let dels = parse_numstat(fields.next())?;
        let path =
            String::from_utf8_lossy(fields.next().context("git numstat record has no path")?)
                .into_owned();
        files.push(FileStat { path, adds, dels });
    }
    Ok(files)
}

/// One line from a commit's patch, or a hunk header when `origin` is `@`.
pub struct PatchLine {
    pub origin: char,
    pub content: String,
}

/// Compare `commit_oid` with its first parent and return the patch for `path`.
pub fn file_patch(
    transaction: &crate::cache::Transaction,
    commit_oid: gix_hash::ObjectId,
    path: &str,
    context_lines: u32,
) -> anyhow::Result<Vec<PatchLine>> {
    let oid = commit_oid.to_string();
    let unified = format!("--unified={context_lines}");
    let output = git_stdout(
        transaction,
        &[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "-p",
            "--no-ext-diff",
            "--no-renames",
            &unified,
            &oid,
            "--",
            path,
        ],
        &[],
    )?;
    let mut in_hunk = false;
    let mut lines = Vec::new();
    for line in output.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"@@") {
            in_hunk = true;
            lines.push(PatchLine {
                origin: '@',
                content: String::from_utf8_lossy(line).into_owned(),
            });
        } else if in_hunk && let Some((&origin, content)) = line.split_first() {
            lines.push(PatchLine {
                origin: origin as char,
                content: String::from_utf8_lossy(content).into_owned(),
            });
        }
    }
    Ok(lines)
}

/// Safely update the worktree to `commit_oid` without overwriting conflicting changes.
pub fn checkout_commit(
    transaction: &crate::cache::Transaction,
    commit_oid: gix_hash::ObjectId,
) -> anyhow::Result<()> {
    let oid = commit_oid.to_string();
    transaction.spawn_git(&["read-tree", "-m", "-u", "HEAD", &oid], &[])?;
    Ok(())
}

/// Whether tracked files differ from the index or worktree.
pub fn has_tracked_changes(transaction: &crate::cache::Transaction) -> anyhow::Result<bool> {
    Ok(!git_stdout(
        transaction,
        &["status", "--porcelain", "--untracked-files=no"],
        &[],
    )?
    .is_empty())
}

/// A reusable reader for Git notes.
pub struct NoteReader {
    repo_path: PathBuf,
}

impl NoteReader {
    pub fn open(repo_path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let repo = gix::open(repo_path.as_ref())?;
        Ok(Self {
            repo_path: repo.path().to_owned(),
        })
    }

    pub fn message(
        &self,
        notes_ref: &str,
        commit_oid: gix_hash::ObjectId,
    ) -> anyhow::Result<String> {
        let output = GitCommand::new(
            &self.repo_path,
            ["notes", "--ref", notes_ref, "show", &commit_oid.to_string()],
            [] as [(&str, &str); 0],
        )
        .with_stdout(std::process::Stdio::piped())
        .spawn()
        .context("missing git note for commit")?;
        String::from_utf8(output.stdout).context("git note is not valid UTF-8")
    }
}

/// Spawn a git command. By default, when used in TTY environment,
/// forwards stdout/stderr to user's TTY
pub struct GitCommand {
    repo_path: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    stderr: Option<std::process::Stdio>,
    stdout: Option<std::process::Stdio>,

    // Temp folder used in tests: used for having predictable output in prysk harnesses
    test_tmp: Option<String>,
}

impl GitCommand {
    pub fn new(
        repo_path: impl AsRef<std::path::Path>,
        args: impl IntoIterator<Item = impl AsRef<str>>,
        env: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
    ) -> GitCommand {
        static TEST_TMP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let test_tmp = TEST_TMP.get_or_init(|| std::env::var("TESTTMP").ok());

        GitCommand {
            repo_path: repo_path.as_ref().into(),
            args: args.into_iter().map(|a| a.as_ref().to_owned()).collect(),
            env: env
                .into_iter()
                .map(|(a, b)| (a.as_ref().to_owned(), b.as_ref().to_owned()))
                .collect(),
            stderr: None,
            stdout: None,
            test_tmp: test_tmp.clone(),
        }
    }

    pub fn with_stdout(mut self, stdout: std::process::Stdio) -> Self {
        self.stdout = Some(stdout);
        self
    }

    pub fn with_stderr(mut self, stderr: std::process::Stdio) -> Self {
        self.stderr = Some(stderr);
        self
    }

    pub fn spawn(self) -> anyhow::Result<std::process::Output> {
        tracing::debug!(args = ?self.args, "spawn");

        // Does not flush any in-memory ODB; callers with a transaction in scope must use
        // `Transaction::spawn_git` instead so the spawned `git` can see in-flight objects.
        let cwd = normalize_repo_path(&self.repo_path);

        let mut command = std::process::Command::new("git");
        command.current_dir(cwd).args(&self.args);

        for (key, value) in self.env {
            command.env(key, value);
        }

        let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

        let stdout = match self.stdout {
            Some(stdout) => stdout,
            None if is_tty => std::process::Stdio::inherit(),
            None => std::process::Stdio::piped(),
        };

        let stderr = match self.stderr {
            Some(stderr) => stderr,
            None if is_tty => std::process::Stdio::inherit(),
            None => std::process::Stdio::piped(),
        };

        command.stdout(stdout);
        command.stderr(stderr);

        let completion = command.output().context("failed to execute git command")?;

        if !is_tty && !completion.stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&completion.stderr);
            let stderr = if let Some(test_tmp) = self.test_tmp {
                stderr.replace(&test_tmp, "${TESTTMP}")
            } else {
                stderr.to_string()
            };

            eprintln!("{}", stderr);
        }

        match completion.status.code().unwrap_or(1) {
            0 => Ok(completion),
            code => {
                let command = self.args.join(" ");
                Err(anyhow!(
                    "Command exited with code {}: git {}",
                    code,
                    command
                ))
            }
        }
    }
}
fn git_stdout(
    transaction: &crate::cache::Transaction,
    args: &[&str],
    env: &[(&str, &str)],
) -> anyhow::Result<Vec<u8>> {
    Ok(transaction
        .git_command(args, env)?
        .with_stdout(std::process::Stdio::piped())
        .spawn()?
        .stdout)
}

fn parse_numstat(field: Option<&[u8]>) -> anyhow::Result<usize> {
    let field = field.context("git numstat record is incomplete")?;
    if field == b"-" {
        return Ok(0);
    }
    std::str::from_utf8(field)?
        .parse()
        .context("git numstat returned an invalid line count")
}

/// Resolve the commit selected by Git's `FETCH_HEAD` pseudo-ref.
pub fn resolve_fetch_head(
    transaction: &crate::cache::Transaction,
) -> anyhow::Result<gix_hash::ObjectId> {
    parse_oid(&git_stdout(
        transaction,
        &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
        &[],
    )?)
}

/// Read a commit's parent OIDs directly from the raw object bytes, parsing only the parent
/// lines via `gix_object::CommitRefIter`; memory-store hits are zero-copy.
pub fn read_parent_ids(
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let (kind, bytes) = odb.read(oid)?;
    // A hard error rather than an assert because repository corruption is user input.
    if kind != gix_object::Kind::Commit {
        return Err(anyhow::anyhow!(
            "object {} is not a commit but a {:?}",
            oid,
            kind
        ));
    }
    Ok(
        gix_object::CommitRefIter::from_bytes(&bytes, gix_hash::Kind::Sha1)
            .parent_ids()
            .collect(),
    )
}

/// Sibling of [`read_parent_ids`]: read a commit's tree OID without touching libgit2's
/// commit parse cache.
pub fn read_tree_id(
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let (kind, bytes) = odb.read(oid)?;
    // Same hard-error rationale as read_parent_ids.
    if kind != gix_object::Kind::Commit {
        return Err(anyhow::anyhow!(
            "object {} is not a commit but a {:?}",
            oid,
            kind
        ));
    }
    Ok(gix_object::CommitRefIter::from_bytes(&bytes, gix_hash::Kind::Sha1).tree_id()?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn read_tree_id_matches_find_commit() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::new("t", "t@example.com", &git2::Time::new(0, 0)).unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let commit_id =
            josh_gix_ext::gix_oid(repo.commit(None, &sig, &sig, "test", &tree, &[]).unwrap());

        let objects_dir = repo.commondir().join("objects");
        let store = josh_memodb::MemOdb::new(None, objects_dir.clone());
        let odb = josh_memodb::Odb::at(store, &objects_dir).unwrap();
        assert_eq!(
            super::read_tree_id(&odb, commit_id).unwrap(),
            crate::objects::gix_oid(
                repo.find_commit(crate::objects::git2_oid(&commit_id))
                    .unwrap()
                    .tree_id()
            )
        );
    }

    #[test]
    fn file_patch_returns_selected_file_hunks() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::new("t", "t@example.com", &git2::Time::new(0, 0)).unwrap();

        let old_blob = repo.blob(b"old\n").unwrap();
        let mut old_builder = repo.treebuilder(None).unwrap();
        old_builder.insert("file", old_blob, 0o100644).unwrap();
        let old_tree = repo.find_tree(old_builder.write().unwrap()).unwrap();
        let parent = repo
            .find_commit(
                repo.commit(None, &sig, &sig, "parent", &old_tree, &[])
                    .unwrap(),
            )
            .unwrap();

        let new_blob = repo.blob(b"new\n").unwrap();
        let mut new_builder = repo.treebuilder(None).unwrap();
        new_builder.insert("file", new_blob, 0o100644).unwrap();
        let new_tree = repo.find_tree(new_builder.write().unwrap()).unwrap();
        let commit = josh_gix_ext::gix_oid(
            repo.commit(None, &sig, &sig, "commit", &new_tree, &[&parent])
                .unwrap(),
        );

        let cache = std::sync::Arc::new(crate::cache::CacheStack::new());
        let transaction = crate::cache::TransactionContext::new(repo.path(), cache)
            .open()
            .unwrap();
        let patch = super::file_patch(&transaction, commit, "file", 3).unwrap();

        assert_eq!(
            patch
                .iter()
                .map(|line| (line.origin, line.content.as_str()))
                .collect::<Vec<_>>(),
            vec![('@', "@@ -1 +1 @@\n"), ('-', "old\n"), ('+', "new\n")]
        );
        assert_eq!(
            super::file_stats(&transaction, commit)
                .unwrap()
                .into_iter()
                .map(|stat| (stat.path, stat.adds, stat.dels))
                .collect::<Vec<_>>(),
            vec![("file".to_owned(), 1, 1)]
        );
        assert!(
            super::file_patch(&transaction, commit, "missing", 3)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn worktree_porcelain_preserves_local_changes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::new("t", "t@example.com", &git2::Time::new(0, 0)).unwrap();
        let path = dir.path().join("file");

        std::fs::write(&path, b"old\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("file")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let old = repo
            .commit(Some("HEAD"), &sig, &sig, "old", &tree, &[])
            .unwrap();

        std::fs::write(&path, b"new\n").unwrap();
        index.add_path(std::path::Path::new("file")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let parent = repo.find_commit(old).unwrap();
        let new = repo
            .commit(Some("HEAD"), &sig, &sig, "new", &tree, &[&parent])
            .unwrap();

        let cache = std::sync::Arc::new(crate::cache::CacheStack::new());
        let transaction = crate::cache::TransactionContext::new(repo.path(), cache)
            .open()
            .unwrap();
        assert!(!super::has_tracked_changes(&transaction).unwrap());

        let old = josh_gix_ext::gix_oid(old);
        super::checkout_commit(&transaction, old).unwrap();
        let head = transaction.head().unwrap();
        transaction
            .update_ref(
                &head.reference,
                crate::cache::Expected::At(josh_gix_ext::gix_oid(new)),
                old,
                "test checkout",
            )
            .unwrap();
        assert!(!super::has_tracked_changes(&transaction).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"old\n");

        std::fs::write(&path, b"local\n").unwrap();
        assert!(super::has_tracked_changes(&transaction).unwrap());
        let snapshot = super::resolve_snapshot_input(&transaction, ".").unwrap();
        let tree = crate::objects::CommitData::read(transaction.odb(), snapshot)
            .unwrap()
            .tree_id()
            .unwrap();
        let entry =
            crate::objects::path_entry(transaction.odb(), tree, std::path::Path::new("file"))
                .unwrap()
                .unwrap();
        assert_eq!(
            crate::objects::blob_text(transaction.odb(), entry.oid),
            "local\n"
        );

        assert!(
            super::checkout_commit(&transaction, josh_gix_ext::gix_oid(new)).is_err(),
            "checkout must not overwrite a conflicting local edit"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"local\n");
    }
}
