use std::path::Path;

use anyhow::{Context, anyhow};
use josh_command_middleware::{Command, CommandStack};
use tokio::sync::{mpsc, oneshot};

use crate::layout::RemoteMeta;

/// Run a `git` command in `repo_path` through the CQ command stack, returning
/// captured stdout on success.
///
/// All git invocations in the merge queue go through the stack so that auth
/// (and any future middleware) is applied uniformly — notably attaching the
/// GitHub token to commands that talk to remotes (`fetch`, `push`).
pub(crate) async fn git_run_command(
    command_env: &CommandStack,
    repo_path: &Path,
    args: &[&str],
) -> anyhow::Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path);
    cmd.args(args.iter().copied());

    let output = command_env
        .run(cmd)
        .await
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "git {} exited with {}: {}",
            args.join(" "),
            output.status,
            stderr.trim()
        ));
    }

    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

/// Reply channel carried by every actor message. The actor sends the
/// operation's result back through it; the caller `await`s the matching
/// receiver. The result is `anyhow::Result` so failures (git2 errors, missing
/// remotes, …) propagate to the caller instead of bringing the actor down.
pub type Reply<T> = oneshot::Sender<anyhow::Result<T>>;

/// Outcome of [`GitActorMessage::PrepareMerge`]: everything the async merge
/// driver needs to run the git subprocess steps (fetch/merge-tree/commit-tree/
/// push) and the follow-up [`GitActorMessage::UnapplyMerge`].
pub struct MergePreparation {
    /// Tracked-remote name the PR belongs to (`remotes/<name>`).
    pub remote_name: String,
    /// Current main of the remote, derived from the metarepo via its workspace
    /// filter — this is the base side of the merge. `main_oid.to_string()` is
    /// the SHA passed to `git merge-tree` / `git commit-tree`.
    pub main_oid: git2::Oid,
    /// OID of the metarepo HEAD commit at preparation time — the pre-image
    /// handed back to `UnapplyMerge`.
    pub head_commit_id: git2::Oid,
    /// Whether the PR head commit is missing locally and must be fetched (via
    /// `RunGitCommand`) before the merge can be computed.
    pub need_fetch: bool,
}

/// Messages handled by the git actor.
///
/// The actor owns the metarepo path and a long-lived [`Transaction`] (git2's
/// `Repository` is `!Send`), and processes these messages serially on a single
/// blocking thread. That serializes all git2 work *and* the git subprocess
/// calls against one another, so on-disk object writes (commit-tree, fetch)
/// are always visible to subsequent git2 reads with no cross-task races.
///
/// Each variant carries a [`Reply`] backchannel; the caller sends the message
/// and awaits the receiver for the result.
///
/// [`Transaction`]: josh_core::cache::Transaction
pub enum GitActorMessage {
    /// Run a `git` subprocess in the metarepo through the CQ command stack (so
    /// the GitHub auth token is attached). Returns captured stdout, erroring on
    /// a non-zero exit. Used for `fetch`, `push`, `merge-tree`, `commit-tree`.
    ///
    /// The actor blocks on the async command-stack call, keeping subprocess
    /// writes ordered against the git2 reads of other messages.
    RunGitCommand {
        args: Vec<String>,
        reply: Reply<String>,
    },

    /// Import a freshly-fetched remote's `FETCH_HEAD` into the metarepo under
    /// `remotes/<id>/contents` and advance HEAD (`track::handle_track`). The
    /// caller must `RunGitCommand` a `fetch <url> HEAD` first so `FETCH_HEAD`
    /// resolves.
    Track {
        url: String,
        id: String,
        reply: Reply<()>,
    },

    /// List every tracked remote from the current metarepo HEAD
    /// (`layout::list_tracked_remotes_for_head`).
    ListTrackedRemotes {
        reply: Reply<Vec<(String, RemoteMeta)>>,
    },

    /// Find the tracked remote whose meta URL matches `url`
    /// (`layout::find_remote_by_url`). Used to test whether a webhook's repo is
    /// tracked and to resolve a PR's remote name.
    FindRemoteByUrl {
        url: String,
        reply: Reply<Option<(String, RemoteMeta)>>,
    },

    /// Resolve a PR into the data needed to merge it — its remote name, the
    /// derived remote main, the metarepo HEAD pre-image, and whether the PR
    /// head must be fetched (step.rs phase 1).
    PrepareMerge {
        repo_url: String,
        head_sha: String,
        reply: Reply<MergePreparation>,
    },

    /// Map a completed merge back onto the metarepo via the remote's workspace
    /// filter and advance HEAD (step.rs phase 2). `main_oid` / `head_commit_id`
    /// come from the matching `PrepareMerge`; `merge_oid` is the commit produced
    /// by `commit-tree`.
    UnapplyMerge {
        remote_name: String,
        head_commit_id: git2::Oid,
        main_oid: git2::Oid,
        merge_oid: git2::Oid,
        reply: Reply<()>,
    },
}

/// Handle to the git actor. Cloneable; every clone targets the same actor.
///
/// Callers build a [`GitActorMessage`] (minus its reply channel) at the call
/// site and pass it to [`GitActor::request`], which wires up the reply channel,
/// sends the message, and awaits the result.
#[derive(Clone)]
pub struct GitActor {
    tx: mpsc::Sender<GitActorMessage>,
}

impl GitActor {
    /// Send a message (built around a fresh reply channel) and await its result.
    ///
    /// `make_msg` receives the reply channel and returns the message to send,
    /// e.g. `git.request(|reply| GitActorMessage::ListTrackedRemotes { reply })`.
    pub(crate) async fn request<T>(
        &self,
        make_msg: impl FnOnce(Reply<T>) -> GitActorMessage,
    ) -> anyhow::Result<T> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(make_msg(reply))
            .await
            .map_err(|_| anyhow!("git actor channel closed"))?;
        rx.await.map_err(|_| anyhow!("git actor dropped reply"))?
    }
}

/// Launch the git actor on a background task, returning a shared [`GitActor`]
/// handle. Wrapped in `Arc` so the single actor is shared, never duplicated.
/// The actor runs until every handle is dropped.
pub fn spawn_git_actor(
    repo_path: std::path::PathBuf,
    cache: std::sync::Arc<josh_core::cache::CacheStack>,
    command_env: CommandStack,
) -> std::sync::Arc<GitActor> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(run_actor_loop(repo_path, cache, command_env, rx));
    std::sync::Arc::new(GitActor { tx })
}

async fn run_actor_loop(
    repo_path: std::path::PathBuf,
    cache: std::sync::Arc<josh_core::cache::CacheStack>,
    command_env: CommandStack,
    mut rx: mpsc::Receiver<GitActorMessage>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            // Only RunGitCommand needs async — the command stack awaits the auth
            // token before spawning git.
            GitActorMessage::RunGitCommand { args, reply } => {
                let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                let result = git_run_command(&command_env, &repo_path, &arg_refs).await;
                let _ = reply.send(result);
            }
            // Everything else is git2 work on the blocking pool. A fresh
            // transaction is opened per event, so the path/cache are cloned in.
            blocking_msg => {
                let repo_path = repo_path.clone();
                let cache = cache.clone();
                tokio::task::spawn_blocking(move || {
                    let transaction = match josh_core::cache::TransactionContext::new(&repo_path, cache).open(None) {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::error!(error = ?e, "git actor failed to open transaction; stopping");
                            return;
                        }
                    };

                    handle_blocking(&transaction, blocking_msg);
                })
                .await
                .expect("git actor blocking task panicked");
            }
        }
    }
}

/// Dispatch a single git2 message, sending its result back through the message's
/// reply channel. Runs inside `spawn_blocking`; never called for
/// `RunGitCommand`.
fn handle_blocking(transaction: &josh_core::cache::Transaction, msg: GitActorMessage) {
    match msg {
        GitActorMessage::Track { url, id, reply } => {
            let _ = reply.send(crate::track::handle_track(&url, &id, transaction));
        }
        GitActorMessage::ListTrackedRemotes { reply } => {
            let result = crate::layout::list_tracked_remotes_for_head(transaction.repo())
                .context("Failed to list tracked remotes");
            let _ = reply.send(result);
        }
        GitActorMessage::FindRemoteByUrl { url, reply } => {
            let result = crate::layout::find_remote_by_url(transaction.repo(), &url)
                .context("Failed to list tracked remotes");
            let _ = reply.send(result);
        }
        GitActorMessage::PrepareMerge {
            repo_url,
            head_sha,
            reply,
        } => {
            let _ = reply.send(prepare_merge(transaction, &repo_url, &head_sha));
        }
        GitActorMessage::UnapplyMerge {
            remote_name,
            head_commit_id,
            main_oid,
            merge_oid,
            reply,
        } => {
            let _ = reply.send(unapply_merge(
                transaction,
                &remote_name,
                head_commit_id,
                main_oid,
                merge_oid,
            ));
        }
        GitActorMessage::RunGitCommand { .. } => {
            unreachable!("RunGitCommand is handled asynchronously in run_actor_loop")
        }
    }
}

/// Resolve a PR into the data needed to merge it (step.rs phase 1): the tracked
/// remote it belongs to, that remote's current main derived from the metarepo,
/// the metarepo HEAD pre-image, and whether the PR head must be fetched.
fn prepare_merge(
    transaction: &josh_core::cache::Transaction,
    repo_url: &str,
    head_sha: &str,
) -> anyhow::Result<MergePreparation> {
    let repo = transaction.repo();
    let (head_commit, _) = crate::layout::head_commit_and_tree(repo)?;

    // Find which tracked remote this PR belongs to.
    let remote_name = crate::layout::find_remote_by_url(repo, repo_url)
        .context("Failed to list tracked remotes")?
        .map(|(name, _)| name)
        .context("No tracked remote found for PR")?;

    // The remote's current main is the metarepo filtered through its workspace.
    let filter = josh_core::filter::parse(&crate::layout::workspace_filter_spec(&remote_name))
        .context("Failed to parse workspace filter")?;
    let main_oid = josh_core::filter::apply_to_commit(filter, &head_commit, transaction)
        .context("Failed to derive remote main")?;
    git_tree_trace::trace_commit(repo, main_oid, "remote main");

    let pr_oid = git2::Oid::from_str(head_sha)?;
    let need_fetch = repo.find_commit(pr_oid).is_err();

    Ok(MergePreparation {
        remote_name,
        main_oid,
        head_commit_id: head_commit.id(),
        need_fetch,
    })
}

/// Map a completed merge back onto the metarepo via the remote's workspace
/// filter and advance HEAD (step.rs phase 2), keeping the metarepo a faithful
/// pre-image of every tracked remote.
fn unapply_merge(
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
    head_commit_id: git2::Oid,
    main_oid: git2::Oid,
    merge_oid: git2::Oid,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    git_tree_trace::trace_commit(repo, merge_oid, "merge");

    let filter = josh_core::filter::parse(&crate::layout::workspace_filter_spec(remote_name))
        .context("Failed to parse workspace filter")?;

    let new_metarepo = josh_core::history::unapply_filter(
        transaction,
        filter,
        head_commit_id,
        main_oid,
        merge_oid,
        josh_core::history::OrphansMode::Keep,
        None,
    )
    .context("Failed to unapply merge onto metarepo")?;

    repo.head()?
        .set_target(new_metarepo, "josh-cq merge")
        .context("Failed to update HEAD")?;

    git_tree_trace::trace_commit(repo, new_metarepo, "metarepo after merge");

    Ok(())
}
