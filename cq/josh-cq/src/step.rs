use anyhow::Context;
use git_tree_trace::trace_commit;
use josh_core::git::{spawn_git_command, spawn_git_command_stdout};
use josh_github_graphql::connection::GithubApiConnection;

use crate::models::CqActorState;

/// Select the first admissible PR from the candidate pool.
///
/// Iterates candidates in insertion order (BTreeMap), checks each one's
/// admission state, and returns the `node_id` of the first that passes
/// `admissible()`. Returns just the id rather than cloning the full struct.
fn select_candidate(state: &CqActorState) -> Option<String> {
    for (node_id, candidate) in &state.candidates {
        if let Some(admission) = state.pr_admissions.get(node_id)
            && admission.admissible()
        {
            tracing::info!(
                pr = %node_id,
                number = candidate.number,
                repo = %candidate.repo_url,
                "selected admissible PR"
            );
            return Some(node_id.clone());
        }
    }
    None
}

/// Run `git merge-tree --write-tree` and return the merged tree OID.
///
/// Returns an error if the output is not a valid 40-char hex string,
/// which indicates a merge conflict or unexpected output.
fn compute_merge_tree(
    repo: &git2::Repository,
    main_sha: &str,
    head_sha: &str,
) -> anyhow::Result<String> {
    let output = spawn_git_command_stdout(
        repo.path(),
        &["merge-tree", "--write-tree", main_sha, head_sha],
    )?;
    // Take the first line — in git >= 2.38 the tree SHA is on line 1
    let merged_tree = output.lines().next().unwrap_or("").trim().to_string();

    if merged_tree.len() != 40 || !merged_tree.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!(
            "merge conflict or unexpected merge-tree output: {}",
            merged_tree
        );
    }

    Ok(merged_tree)
}

/// Create a merge commit via `git commit-tree`.
fn create_merge_commit(
    repo: &git2::Repository,
    main_sha: &str,
    head_sha: &str,
    merged_tree: &str,
    message: &str,
) -> anyhow::Result<String> {
    let output = spawn_git_command_stdout(
        repo.path(),
        &[
            "commit-tree",
            "-p",
            main_sha,
            "-p",
            head_sha,
            "-m",
            message,
            merged_tree,
        ],
    )?;
    Ok(output.trim().to_string())
}

/// Force-push the merge commit to the remote's base branch.
///
/// The push is forced because the metarepo is the source of truth: anything
/// pushed to the remote outside the queue is overwritten.
fn push_to_remote(
    repo: &git2::Repository,
    remote_url: &str,
    merge_commit: &str,
    base_branch: &str,
) -> anyhow::Result<()> {
    let target_ref = format!("refs/heads/{}", base_branch);
    let refspec = format!("+{}:{}", merge_commit, target_ref);
    spawn_git_command(repo.path(), &["push", remote_url, &refspec], &[])?;
    Ok(())
}

/// Post a merge comment and close the PR on GitHub.
/// No-ops silently when `api` is `None` (e.g. in test environments).
async fn close_pr_on_github(
    api: Option<&GithubApiConnection>,
    node_id: &str,
    merge_commit: &str,
    _number: i64,
    _title: &str,
) -> anyhow::Result<()> {
    let Some(api) = api else {
        return Ok(());
    };
    let comment = format!("Merged by Josh merge queue as `{}`.", merge_commit);
    api.add_pr_comment(node_id, &comment).await?;
    api.close_pull_request(node_id).await?;
    Ok(())
}

/// Merge an admissible PR: compute merge locally, push to remote main,
/// unapply the merge back onto the metarepo, close the PR, and remove from the
/// candidate pool.
async fn handle_step(
    node_id: &str,
    transaction: josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    let (repo_url, head_sha, base_branch, number, title) = {
        let candidate = state
            .get_candidate(node_id)
            .context("candidate not found in state")?;
        (
            candidate.repo_url.clone(),
            candidate.head_sha.clone(),
            candidate.base_branch.clone(),
            candidate.number,
            candidate.title.clone(),
        )
    };

    let node_id_owned = node_id.to_string();
    let title_for_close = title.clone();

    let merge_commit: Option<String> = tokio::task::spawn_blocking(move || {
        let repo = transaction.repo();
        let head_commit = repo
            .head()
            .context("Failed to get HEAD")?
            .peel_to_commit()
            .context("Failed to peel HEAD to commit")?;
        let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

        // Find which tracked remote this PR belongs to.
        let name = crate::layout::list_tracked_remotes(repo, &head_tree)
            .context("Failed to list tracked remotes")?
            .into_iter()
            .find(|(_, meta)| meta.url == repo_url)
            .map(|(name, _)| name)
            .context("No tracked remote found for PR")?;

        // The remote's current main is the metarepo filtered through its workspace.
        let filter = josh_core::filter::parse(&crate::layout::workspace_filter_spec(&name))
            .context("Failed to parse workspace filter")?;
        let main_oid = josh_core::filter::apply_to_commit(filter, &head_commit, &transaction)
            .context("Failed to derive remote main")?;
        let main_sha = main_oid.to_string();
        trace_commit(repo, main_oid, "remote main");

        let pr_oid = git2::Oid::from_str(&head_sha)?;
        if repo.find_commit(pr_oid).is_err() {
            spawn_git_command(repo.path(), &["fetch", &repo_url, &head_sha], &[])?;
        }
        trace_commit(repo, pr_oid, "PR head");

        let merged_tree = match compute_merge_tree(repo, &main_sha, &head_sha) {
            Ok(tree) => tree,
            Err(_) => {
                tracing::warn!(
                    pr = %node_id_owned,
                    "merge conflict detected; skipping PR"
                );
                return Ok::<_, anyhow::Error>(None);
            }
        };

        let message = format!("Merge PR #{}: {}", number, title);
        let merge_commit = create_merge_commit(repo, &main_sha, &head_sha, &merged_tree, &message)?;
        let merge_oid = merge_commit
            .parse::<git2::Oid>()
            .context("Failed to parse merge commit OID")?;
        trace_commit(repo, merge_oid, "merge");

        // Push the merge to the remote's main branch.
        push_to_remote(repo, &repo_url, &merge_commit, &base_branch)?;

        // Map the merge back onto the metarepo and advance HEAD, so the metarepo
        // stays a faithful pre-image of every tracked remote.
        let new_metarepo = josh_core::history::unapply_filter(
            &transaction,
            filter,
            head_commit.id(),
            main_oid,
            merge_oid,
            josh_core::history::OrphansMode::Keep,
            None,
        )
        .context("Failed to unapply merge onto metarepo")?;
        repo.head()?
            .set_target(new_metarepo, "josh-cq merge")
            .context("Failed to update HEAD")?;
        trace_commit(repo, new_metarepo, "metarepo after merge");

        Ok::<_, anyhow::Error>(Some(merge_commit))
    })
    .await??;

    let merge_commit = match merge_commit {
        Some(mc) => mc,
        None => {
            state.remove_candidate(node_id);
            return Ok(());
        }
    };

    close_pr_on_github(api, node_id, &merge_commit, number, &title_for_close).await?;

    state.remove_candidate(node_id);

    Ok(())
}

/// Run evaluate→step while admissible PRs remain.
/// Called after every event (webhook or tick) to try to make progress.
pub(crate) async fn run_queue_cycle(
    state: &mut CqActorState,
    repo_path: &std::path::Path,
    cache: &std::sync::Arc<josh_core::cache::CacheStack>,
    api: Option<&GithubApiConnection>,
) {
    use josh_core::cache::TransactionContext;

    loop {
        let node_id = match select_candidate(state) {
            Some(id) => id,
            None => {
                tracing::debug!("no admissible PRs");
                break;
            }
        };

        // Snapshot candidate fields for logging before handle_step, which
        // removes the candidate from state on success.
        let (log_number, log_repo) = {
            let c = state
                .get_candidate(&node_id)
                .expect("candidate must exist after select_candidate");
            (c.number, c.repo_url.clone())
        };

        let transaction = match TransactionContext::new(repo_path, cache.clone()).open(None) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(
                    error = ?e,
                    "failed to open transaction for merge"
                );
                break;
            }
        };

        match handle_step(&node_id, transaction, api, state).await {
            Ok(()) => {
                tracing::info!(
                    pr = %node_id,
                    number = log_number,
                    repo = %log_repo,
                    "merged PR"
                );
            }
            Err(e) => {
                tracing::error!(
                    pr = %node_id,
                    number = log_number,
                    error = ?e,
                    "failed to merge PR; will retry next cycle"
                );
                break;
            }
        }
    }
}
