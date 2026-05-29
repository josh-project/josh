use std::collections::BTreeSet;

use anyhow::Context;

use josh_core::git::spawn_git_command;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;

use crate::models::{CandidatePr, CqActorState};

pub(crate) async fn fetch_maintainers(
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
    match api.get_maintainers(&owner, &name).await {
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

pub(crate) async fn lookup_open_prs_by_sha(
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
    match api.find_open_prs_by_head_sha(&owner, &name, sha).await {
        Ok(prs) => prs.into_iter().map(|(id, _)| id).collect(),
        Err(e) => {
            tracing::warn!(url = %clone_url, sha = %sha, error = ?e, "failed to look up PRs by SHA");
            Vec::new()
        }
    }
}

pub(crate) async fn handle_fetch(
    transaction: josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    // Enumerate tracked remotes from the metarepo and fetch their objects so
    // PR head commits are available locally. The remote's main is derived from
    // the metarepo (see step.rs), so nothing is written back here.
    let remotes: Vec<String> = tokio::task::spawn_blocking(move || {
        let repo = transaction.repo();
        let head_tree = repo
            .head()
            .context("Failed to get HEAD")?
            .peel_to_commit()
            .context("Failed to peel HEAD to commit")?
            .tree()
            .context("Failed to get HEAD tree")?;

        let tracked = crate::layout::list_tracked_remotes(repo, &head_tree)
            .context("Failed to list tracked remotes")?;

        if tracked.is_empty() {
            tracing::info!("no tracked remotes found");
            return Ok::<_, anyhow::Error>(Vec::new());
        }

        let mut urls = Vec::with_capacity(tracked.len());
        for (_, meta) in &tracked {
            spawn_git_command(repo.path(), &["fetch", meta.url.as_str()], &[])
                .with_context(|| format!("Failed to fetch from {}", meta.url))?;
            urls.push(meta.url.clone());
        }

        Ok::<_, anyhow::Error>(urls)
    })
    .await??;

    for url in &remotes {
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

        let prs = match api.get_open_pull_requests(&owner, &repo_name).await {
            Ok(prs) => prs,
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "failed to fetch open PRs");
                continue;
            }
        };

        // Reconcile required checks against the current rulesets. A PR's
        // admission (and its required-check set) may have been initialized from
        // a webhook before a ruleset existed; tick is the reconciliation point.
        let required = match fetch_required_checks(api, &owner, &repo_name).await {
            Ok(checks) => {
                state.admission.insert(url.clone(), checks.clone());
                Some(checks)
            }
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "failed to refresh required checks");
                None
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

            state
                .get_or_init_pr_admission(&pr.node_id, url, Some(api))
                .await;

            // Bring the PR's required checks in line with the current rulesets,
            // preserving already-known pass/fail results.
            if let Some(required) = &required
                && let Some(admission) = state.pr_admissions.get_mut(&pr.node_id)
            {
                admission
                    .required_checks
                    .retain(|c, _| required.contains(c));
                for check in required {
                    admission
                        .required_checks
                        .entry(check.clone())
                        .or_insert(false);
                }
            }

            match api.get_pr_reviews(&owner, &repo_name, pr.number).await {
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

    Ok(())
}
