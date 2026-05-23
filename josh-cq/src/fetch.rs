use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Context;

use josh_core::git::spawn_git_command;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;

use crate::models::{CandidatePr, CqActorState};

pub(crate) fn fetch_maintainers(
    clone_url: &str,
    api: Option<&GithubApiConnection>,
    state: &CqActorState,
) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let (owner, name) = match state.resolve_owner_repo(clone_url) {
        Some(parts) => parts,
        None => return Vec::new(),
    };
    match tokio::runtime::Handle::current().block_on(api.get_maintainers(&owner, &name)) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(url = %clone_url, error = ?e, "failed to fetch maintainers");
            Vec::new()
        }
    }
}

pub(crate) async fn fetch_required_checks(
    api: &GithubApiConnection,
    owner: &str,
    name: &str,
) -> anyhow::Result<BTreeSet<RequiredStatusCheck>> {
    let rulesets = api.get_repository_rulesets(owner, name).await?;
    let mut checks = BTreeSet::new();
    for ruleset in rulesets {
        if !ruleset.is_active() {
            continue;
        }
        match api.get_ruleset_required_checks(&ruleset.id).await {
            Ok(rs_checks) => checks.extend(rs_checks),
            Err(e) => tracing::warn!(
                ruleset = %ruleset.id,
                error = ?e,
                "failed to fetch checks for ruleset; skipping"
            ),
        }
    }

    Ok(checks)
}

pub(crate) fn lookup_open_prs_by_sha(
    api: Option<&GithubApiConnection>,
    clone_url: &str,
    sha: &str,
    state: &CqActorState,
) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let (owner, name) = match state.resolve_owner_repo(clone_url) {
        Some(parts) => parts,
        None => {
            tracing::warn!(url = %clone_url, "could not resolve owner/repo");
            return Vec::new();
        }
    };
    match tokio::runtime::Handle::current()
        .block_on(api.find_open_prs_by_head_sha(&owner, &name, sha))
    {
        Ok(prs) => prs.into_iter().map(|(id, _)| id).collect(),
        Err(e) => {
            tracing::warn!(url = %clone_url, sha = %sha, error = ?e, "failed to look up PRs by SHA");
            Vec::new()
        }
    }
}

pub(crate) fn handle_fetch(
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    mut state: CqActorState,
) -> anyhow::Result<CqActorState> {
    let repo = transaction.repo();
    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let mut remotes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, filter) in &link_files {
        if let (Some(remote), Some(commit)) = (filter.get_meta("remote"), filter.get_meta("commit"))
        {
            remotes.push((path.clone(), remote, commit));
        }
    }

    if remotes.is_empty() {
        tracing::info!("no tracked remotes found");
        return Ok(state);
    }

    let signature = josh_link::make_signature(repo)?;
    let mut links_to_update: Vec<(PathBuf, git2::Oid)> = Vec::new();

    for (path, url, current_commit) in &remotes {
        spawn_git_command(repo.path(), &["fetch", url.as_str()], &[])
            .with_context(|| format!("Failed to fetch from {}", url))?;

        let refs = crate::remote::list_refs(url)
            .with_context(|| format!("Failed to list refs for {}", url))?;

        if let Some(head_oid) = refs.get("HEAD") {
            if head_oid.to_string() != *current_commit {
                links_to_update.push((path.clone(), *head_oid));
            }
        }
    }

    if !links_to_update.is_empty() {
        let count = links_to_update.len();
        match josh_link::update_links(repo, transaction, &head_commit, links_to_update, &signature)?
        {
            Some(result) => {
                repo.head()?
                    .set_target(result.commit_with_updates, "josh-cq fetch")
                    .context("Failed to update HEAD")?;
            }
            None => {
                tracing::debug!("link files already up to date");
            }
        }
        tracing::info!(count, "updated link file(s)");
    }

    for (_, url, _) in &remotes {
        let (owner, repo_name) = match state.resolve_owner_repo(url) {
            Some(parts) => parts,
            None => {
                tracing::warn!(url = %url, "could not resolve owner/repo");
                continue;
            }
        };

        let Some(api) = api else {
            tracing::warn!(url = %url, "skipping PR discovery: no API connection");
            continue;
        };

        let prs = match tokio::runtime::Handle::current()
            .block_on(api.get_open_pull_requests(&owner, &repo_name))
        {
            Ok(prs) => prs,
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "failed to fetch open PRs");
                continue;
            }
        };

        for pr in &prs {
            if state.closed_prs.contains(&pr.node_id) {
                continue;
            }
            state.upsert_candidate(CandidatePr {
                node_id: pr.node_id.clone(),
                number: pr.number,
                repo_url: url.clone(),
                head_sha: pr.head_sha.clone(),
                head_branch: pr.head_branch.clone(),
                base_sha: pr.base_sha.clone(),
                base_branch: pr.base_branch.clone(),
                title: pr.title.clone(),
            });

            state.get_or_init_pr_admission(&pr.node_id, url, Some(api));

            match tokio::runtime::Handle::current()
                .block_on(api.get_pr_reviews(&owner, &repo_name, pr.number))
            {
                Ok(reviews) => {
                    if let Some(admission) = state.pr_admissions.get_mut(&pr.node_id) {
                        admission.apply_review_states(&reviews);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        pr = %pr.node_id,
                        error = ?e,
                        "failed to fetch PR reviews"
                    );
                }
            }
        }

        tracing::info!(
            url = %url,
            count = prs.len(),
            "discovered open PRs"
        );
    }

    Ok(state)
}
