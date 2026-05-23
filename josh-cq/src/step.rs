use std::path::PathBuf;

use anyhow::Context;

use josh_core::git::{spawn_git_command, spawn_git_command_stdout};
use josh_github_graphql::connection::GithubApiConnection;
use josh_link::make_signature;

use crate::models::{CandidatePr, CqActorState};

/// Select the first admissible PR from the candidate pool.
///
/// Iterates candidates in insertion order (BTreeMap), checks each one's
/// admission state, and returns the first that passes `admissible()`.
fn select_candidate(state: &CqActorState) -> Option<CandidatePr> {
    for (node_id, candidate) in &state.candidates {
        if let Some(admission) = state.pr_admissions.get(node_id) {
            if admission.admissible() {
                tracing::info!(
                    pr = %node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "selected admissible PR"
                );
                return Some(candidate.clone());
            }
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

/// Push the merge commit to the remote's base branch.
fn push_to_remote(
    repo: &git2::Repository,
    remote_url: &str,
    merge_commit: &str,
    base_branch: &str,
) -> anyhow::Result<()> {
    let target_ref = format!("refs/heads/{}", base_branch);
    let refspec = format!("{}:{}", merge_commit, target_ref);
    spawn_git_command(repo.path(), &["push", remote_url, &refspec], &[])?;
    Ok(())
}

/// Update `.link.josh` in the metarepo and set HEAD to the resulting commit.
fn update_metarepo_links(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    head_commit: &git2::Commit,
    link_path: PathBuf,
    merge_oid: git2::Oid,
    signature: &git2::Signature,
) -> anyhow::Result<()> {
    match josh_link::update_links(
        repo,
        transaction,
        head_commit,
        vec![(link_path, merge_oid)],
        signature,
    )? {
        Some(result) => {
            repo.head()?
                .set_target(result.commit_with_updates, "josh-cq merge")
                .context("Failed to update HEAD")?;
        }
        None => {
            tracing::debug!("link file already up to date");
        }
    }
    Ok(())
}

/// Post a merge comment and close the PR on GitHub.
/// No-ops silently when `api` is `None` (e.g. in test environments).
fn close_pr_on_github(
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
    tokio::runtime::Handle::current().block_on(async {
        api.add_pr_comment(node_id, &comment).await?;
        api.close_pull_request(node_id).await
    })?;
    Ok(())
}

/// Merge an admissible PR: compute merge locally, push to remote main,
/// update `.link.josh`, close the PR, and remove from the candidate pool.
fn handle_step(
    candidate: &CandidatePr,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let mut main_sha = None;
    let mut link_path = None;
    for (path, filter) in &link_files {
        if filter.get_meta("remote").as_deref() == Some(candidate.repo_url.as_str()) {
            main_sha = filter.get_meta("commit");
            link_path = Some(path.clone());
            break;
        }
    }

    let main_sha = main_sha.context("No link file found for remote")?;
    let link_path = link_path.context("No link file found for remote")?;

    // Fetch the PR head commit if we don't have it locally yet
    if repo
        .find_commit(git2::Oid::from_str(&candidate.head_sha)?)
        .is_err()
    {
        spawn_git_command(
            repo.path(),
            &["fetch", &candidate.repo_url, &candidate.head_sha],
            &[],
        )?;
    }

    let merged_tree = match compute_merge_tree(repo, &main_sha, &candidate.head_sha) {
        Ok(tree) => tree,
        Err(_) => {
            tracing::warn!(
                pr = %candidate.node_id,
                "merge conflict detected; skipping PR"
            );
            state.remove_candidate(&candidate.node_id);
            return Ok(());
        }
    };

    let message = format!("Merge PR #{}: {}", candidate.number, candidate.title);
    let merge_commit =
        create_merge_commit(repo, &main_sha, &candidate.head_sha, &merged_tree, &message)?;

    push_to_remote(
        repo,
        &candidate.repo_url,
        &merge_commit,
        &candidate.base_branch,
    )?;

    let merge_oid = merge_commit
        .parse::<git2::Oid>()
        .context("Failed to parse merge commit OID")?;
    let signature = make_signature(repo)?;
    update_metarepo_links(
        repo,
        transaction,
        &head_commit,
        link_path,
        merge_oid,
        &signature,
    )?;

    close_pr_on_github(
        api,
        &candidate.node_id,
        &merge_commit,
        candidate.number,
        &candidate.title,
    )?;

    state.remove_candidate(&candidate.node_id);

    Ok(())
}

/// Run evaluate→step while admissible PRs remain.
/// Called after every event (webhook or tick) to try to make progress.
pub(crate) fn run_queue_cycle(
    state: &mut CqActorState,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
) {
    loop {
        let candidate = match select_candidate(state) {
            Some(c) => c,
            None => {
                tracing::debug!("no admissible PRs");
                break;
            }
        };

        match handle_step(&candidate, transaction, api, state) {
            Ok(()) => {
                tracing::info!(
                    pr = %candidate.node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "merged PR"
                );
            }
            Err(e) => {
                tracing::error!(
                    pr = %candidate.node_id,
                    number = candidate.number,
                    error = ?e,
                    "failed to merge PR; will retry next cycle"
                );
                break;
            }
        }
    }
}
