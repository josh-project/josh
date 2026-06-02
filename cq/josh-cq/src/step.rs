use anyhow::Context;
use josh_github_graphql::connection::GithubApiConnection;

use crate::git::{GitActor, GitActorMessage};
use crate::models::CqActorState;

/// Select the first admissible PR from the candidate pool.
///
/// Iterates candidates in insertion order (BTreeMap), checks each one's
/// admission state, and returns the `node_id` of the first that passes
/// `admissible()`. Returns just the id rather than cloning the full struct.
fn select_candidate(state: &CqActorState) -> Option<String> {
    for (node_id, candidate) in &state.candidates {
        if let Some(admission) = state.admissions.get(node_id)
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
async fn compute_merge_tree(
    git: &GitActor,
    main_sha: &str,
    head_sha: &str,
) -> anyhow::Result<String> {
    let output = git
        .request(|reply| GitActorMessage::RunGitCommand {
            args: vec![
                "merge-tree".to_string(),
                "--write-tree".to_string(),
                main_sha.to_string(),
                head_sha.to_string(),
            ],
            reply,
        })
        .await?;
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
async fn create_merge_commit(
    git: &GitActor,
    main_sha: &str,
    head_sha: &str,
    merged_tree: &str,
    message: &str,
) -> anyhow::Result<String> {
    let output = git
        .request(|reply| GitActorMessage::RunGitCommand {
            args: vec![
                "commit-tree".to_string(),
                "-p".to_string(),
                main_sha.to_string(),
                "-p".to_string(),
                head_sha.to_string(),
                "-m".to_string(),
                message.to_string(),
                merged_tree.to_string(),
            ],
            reply,
        })
        .await?;
    Ok(output.trim().to_string())
}

/// Force-push the merge commit to the remote's base branch.
///
/// The push is forced because the metarepo is the source of truth: anything
/// pushed to the remote outside the queue is overwritten.
async fn push_to_remote(
    git: &GitActor,
    remote_url: &str,
    merge_commit: &str,
    base_branch: &str,
) -> anyhow::Result<()> {
    let target_ref = format!("refs/heads/{}", base_branch);
    let refspec = format!("+{}:{}", merge_commit, target_ref);
    git.request(|reply| GitActorMessage::RunGitCommand {
        args: vec!["push".to_string(), remote_url.to_string(), refspec],
        reply,
    })
    .await?;
    Ok(())
}

/// Post a merge comment and close the PR on GitHub.
/// No-ops silently when `api` is `None` (e.g. in test environments).
async fn close_pr_on_github(
    api: &GithubApiConnection,
    node_id: &str,
    merge_commit: &str,
) -> anyhow::Result<()> {
    let comment = format!("Merged by Josh merge queue as `{}`.", merge_commit);
    api.add_pr_comment(node_id, &comment).await?;
    api.close_pull_request(node_id).await?;
    Ok(())
}

/// Merge an admissible PR: compute merge locally, push to remote main,
/// unapply the merge back onto the metarepo, close the PR, and remove from the
/// candidate pool.
///
/// All git work — the git2 phases (`PrepareMerge`/`UnapplyMerge`) and the git
/// subprocess steps (fetch/merge-tree/commit-tree/push) — goes through the git
/// actor, which serializes them and attaches the auth token.
async fn handle_step(
    node_id: &str,
    git: &GitActor,
    api: &GithubApiConnection,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    let candidate = state
        .get_candidate(node_id)
        .context("candidate not found in state")?
        .clone();

    // Resolve the tracked remote and derive its current main from the metarepo.
    let prep = git
        .request(|reply| GitActorMessage::PrepareMerge {
            repo_url: candidate.repo_url.clone(),
            head_sha: candidate.head_sha.clone(),
            reply,
        })
        .await?;

    // Fetch the PR head if it isn't present locally.
    if prep.need_fetch {
        git.request(|reply| GitActorMessage::RunGitCommand {
            args: vec![
                "fetch".to_string(),
                candidate.repo_url.clone(),
                candidate.head_sha.clone(),
            ],
            reply,
        })
        .await?;
    }

    let main_sha = prep.main_oid.to_string();
    let merged_tree = match compute_merge_tree(git, &main_sha, &candidate.head_sha).await {
        Ok(tree) => tree,
        Err(_) => {
            tracing::warn!(pr = %node_id, "merge conflict detected; skipping PR");
            state.remove_candidate(node_id);
            return Ok(());
        }
    };

    let message = format!("Merge PR #{}: {}", candidate.number, candidate.title);
    let merge_commit =
        create_merge_commit(git, &main_sha, &candidate.head_sha, &merged_tree, &message).await?;
    let merge_oid = merge_commit
        .parse::<git2::Oid>()
        .context("Failed to parse merge commit OID")?;

    // Push the merge to the remote's main branch.
    push_to_remote(
        git,
        &candidate.repo_url,
        &merge_commit,
        &candidate.base_branch,
    )
    .await?;

    // Map the merge back onto the metarepo and advance HEAD, so the metarepo
    // stays a faithful pre-image of every tracked remote.
    git.request(|reply| GitActorMessage::UnapplyMerge {
        remote_name: prep.remote_name,
        head_commit_id: prep.head_commit_id,
        main_oid: prep.main_oid,
        merge_oid,
        reply,
    })
    .await?;

    close_pr_on_github(api, node_id, &merge_commit).await?;

    state.remove_candidate(node_id);

    Ok(())
}

/// Run evaluate→step while admissible PRs remain.
/// Called after every event (webhook or tick) to try to make progress.
pub(crate) async fn run_queue_cycle(
    state: &mut CqActorState,
    git: &GitActor,
    api: &GithubApiConnection,
) {
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

        match handle_step(&node_id, git, api, state).await {
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
