use anyhow::{Context, anyhow};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum CommandColor {
    Auto = 0,
    Always = 1,
    Never = 2,
}

static MACHINE_OUTPUT: AtomicBool = AtomicBool::new(false);
static QUIET_OUTPUT: AtomicBool = AtomicBool::new(false);
static NO_PROGRESS: AtomicBool = AtomicBool::new(false);
static NON_INTERACTIVE: AtomicBool = AtomicBool::new(false);
static COMMAND_COLOR: AtomicU8 = AtomicU8::new(CommandColor::Auto as u8);

/// Configure how Git subprocesses interact with the terminal.
///
/// The CLI calls this once after parsing its global output options. Atomic storage keeps the
/// core independent from the CLI and also makes repeated configuration safe in tests.
pub fn configure_command_output(
    machine: bool,
    quiet: bool,
    no_progress: bool,
    non_interactive: bool,
    color: CommandColor,
) {
    MACHINE_OUTPUT.store(machine, Ordering::Relaxed);
    QUIET_OUTPUT.store(quiet, Ordering::Relaxed);
    NO_PROGRESS.store(no_progress, Ordering::Relaxed);
    NON_INTERACTIVE.store(non_interactive, Ordering::Relaxed);
    COMMAND_COLOR.store(color as u8, Ordering::Relaxed);
}

/// Resolve the `input_ref` argument to a commit OID.
///
/// - `"+"`: Creates a temporary commit from the current index (staged changes
///   on top of HEAD).
/// - `"."`: Creates a temporary commit from the working tree (all tracked and
///   untracked files under the repo root).
/// - A raw SHA hex string: resolves the object and peels to its commit.
/// - Anything else: treated as a ref name.
pub fn resolve_snapshot_input(
    repo: &git2::Repository,
    input_ref: &str,
) -> anyhow::Result<git2::Oid> {
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
        Ok(commit_oid)
    } else if let Ok(oid) = git2::Oid::from_str(input_ref) {
        Ok(repo.find_object(oid, None)?.peel_to_commit()?.id())
    } else {
        let obj = repo
            .revparse_single(input_ref)
            .with_context(|| format!("could not resolve input: {:?}", input_ref))?;
        Ok(obj.peel_to_commit()?.id())
    }
}

const JOSH_COMMIT_TIME_ENV: &str = "JOSH_COMMIT_TIME";
const JOSH_COMMIT_NAME: &str = "JOSH";
const JOSH_COMMIT_EMAIL: &str = "josh@josh-project.dev";

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

/// Parse a date string from `GIT_COMMITTER_DATE` / `GIT_AUTHOR_DATE`. Accepts the
/// formats git typically uses: raw (`<unix> <offset>`), RFC 2822 (what `date -R`
/// emits) and RFC 3339 / ISO 8601.
fn parse_git_env_date(s: &str) -> Option<git2::Time> {
    let s = s.trim();
    if let Some((secs, offset)) = s.split_once(' ') {
        if let (Ok(secs), Ok(offset)) = (secs.parse::<i64>(), offset.parse::<i32>()) {
            let offset_minutes = (offset / 100) * 60 + (offset % 100);
            return Some(git2::Time::new(secs, offset_minutes));
        }
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(git2::Time::new(
            dt.timestamp(),
            dt.offset().local_minus_utc() / 60,
        ));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(git2::Time::new(
            dt.timestamp(),
            dt.offset().local_minus_utc() / 60,
        ));
    }
    None
}

/// Like `repo.signature()` but honors `GIT_COMMITTER_*` / `GIT_AUTHOR_*` env vars
/// the way `git` itself does. libgit2's `git_signature_default` ignores the date
/// env vars, which breaks reproducibility in tests.
pub fn user_signature(repo: &git2::Repository) -> anyhow::Result<git2::Signature<'static>> {
    let default = repo.signature()?;
    let name = std::env::var("GIT_COMMITTER_NAME")
        .or_else(|_| std::env::var("GIT_AUTHOR_NAME"))
        .ok();
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .or_else(|_| std::env::var("GIT_AUTHOR_EMAIL"))
        .ok();
    let date = std::env::var("GIT_COMMITTER_DATE")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok());
    let date = date.as_deref().and_then(parse_git_env_date);

    let name = name
        .as_deref()
        .unwrap_or_else(|| default.name().unwrap_or(""));
    let email = email
        .as_deref()
        .unwrap_or_else(|| default.email().unwrap_or(""));
    let time = date.unwrap_or_else(|| default.when());
    Ok(git2::Signature::new(name, email, &time)?)
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

/// Spawn a git command directly to the terminal so users can see progress
/// Falls back to captured output if not in a TTY environment
pub fn spawn_git_command(
    repo_path: &std::path::Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> anyhow::Result<()> {
    log::debug!("spawn_git_command: {:?}", args);

    // Does not flush any in-memory ODB; callers with a transaction in scope must use
    // `Transaction::spawn_git` instead so the spawned `git` can see in-flight objects.
    let cwd = normalize_repo_path(repo_path);

    let machine = MACHINE_OUTPUT.load(Ordering::Relaxed);
    let quiet = QUIET_OUTPUT.load(Ordering::Relaxed);
    let no_progress = NO_PROGRESS.load(Ordering::Relaxed);
    let non_interactive = NON_INTERACTIVE.load(Ordering::Relaxed);
    let color = COMMAND_COLOR.load(Ordering::Relaxed);

    let mut command = std::process::Command::new("git");
    command.current_dir(cwd);

    for (key, value) in env {
        command.env(key, value);
    }
    if non_interactive {
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never");
    }
    match color {
        value if value == CommandColor::Always as u8 => {
            command
                .args(["-c", "color.ui=always"])
                .env("CLICOLOR_FORCE", "1");
        }
        value if value == CommandColor::Never as u8 => {
            command
                .args(["-c", "color.ui=false"])
                .env("NO_COLOR", "1")
                .env("TERM", "dumb");
        }
        _ => {}
    }
    command.args(args);

    // Machine mode always captures child output so stdout remains a valid JSON document.
    let is_tty = std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && !machine
        && !quiet
        && !no_progress;
    let mut captured_stderr = String::new();

    let status = if is_tty {
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        command.status()?.code()
    } else {
        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("failed to execute git command")?;

        if !output.stderr.is_empty() {
            let output_str = String::from_utf8_lossy(&output.stderr);
            captured_stderr = if let Ok(testtmp) = std::env::var("TESTTMP") {
                output_str.replace(&testtmp, "${TESTTMP}")
            } else {
                output_str.to_string()
            };

            if !machine && !quiet && !no_progress {
                eprintln!("{}", captured_stderr);
            }
        }

        output.status.code()
    };

    match status.unwrap_or(1) {
        0 => Ok(()),
        code => {
            let command = args.join(" ");
            let error = anyhow!("Command exited with code {}: git {}", code, command);
            if (machine || quiet || no_progress) && !captured_stderr.trim().is_empty() {
                Err(anyhow!(captured_stderr.trim().to_string()).context(error.to_string()))
            } else {
                Err(error)
            }
        }
    }
}
