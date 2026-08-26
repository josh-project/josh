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
    let repo = transaction.git2_repo();
    if input_ref == "+" || input_ref == "." {
        let mut index = repo.index()?;
        let tree_oid = if input_ref == "+" {
            index.write_tree_to(repo)?
        } else {
            let head_tree = repo.head()?.peel_to_tree()?;
            index.read_tree(&head_tree)?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.update_all(["*"].iter(), None)?;
            index.write_tree_to(repo)?
        };
        let tree = repo.find_tree(tree_oid)?;
        let sig = crate::git::josh_commit_signature()?;
        let head_commit = repo.head()?.peel_to_commit()?;
        let commit_oid = repo.commit(None, &sig, &sig, "WIP", &tree, &[&head_commit])?;
        Ok(crate::objects::gix_oid(commit_oid))
    } else if let Ok(oid) = gix_hash::ObjectId::from_str(input_ref) {
        Ok(crate::objects::gix_oid(
            repo.find_object(crate::objects::git2_oid(&oid), None)?
                .peel_to_commit()?
                .id(),
        ))
    } else {
        let obj = repo
            .revparse_single(input_ref)
            .with_context(|| format!("could not resolve input: {:?}", input_ref))?;
        Ok(crate::objects::gix_oid(obj.peel_to_commit()?.id()))
    }
}

const JOSH_COMMIT_TIME_ENV: &str = "JOSH_COMMIT_TIME";
const JOSH_COMMIT_NAME: &str = "JOSH";
const JOSH_COMMIT_EMAIL: &str = "josh@josh-project.dev";

/// The libgit2 spelling of Josh's fixed commit identity, for porcelain and fixture seams.
pub fn josh_commit_signature<'a>() -> anyhow::Result<git2::Signature<'a>> {
    Ok(if let Ok(time) = std::env::var(JOSH_COMMIT_TIME_ENV) {
        git2::Signature::new(
            JOSH_COMMIT_NAME,
            JOSH_COMMIT_EMAIL,
            &git2::Time::new(time.parse()?, 0),
        )?
    } else {
        git2::Signature::now(JOSH_COMMIT_NAME, JOSH_COMMIT_EMAIL)?
    })
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
    if let Ok(repo) = git2::Repository::open(repo_path)
        && let Some(workdir) = repo.workdir()
    {
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

/// Read a commit's parent OIDs directly from the raw object bytes, parsing only the parent
/// lines via `gix_object::CommitRefIter`. This avoids libgit2's commit parse cache (a
/// lock-guarded global that also decodes author/committer/message) whenever a caller only
/// needs the parent ids; memory-store hits are zero-copy.
pub fn read_parent_ids(
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let (kind, bytes) = odb.read(oid)?;
    // A hard error, not an assert: this is reachable from inside git2 callback frames,
    // where unwinding across the FFI boundary would abort.
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
            crate::objects::gix_oid(repo.commit(None, &sig, &sig, "test", &tree, &[]).unwrap());

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
}
