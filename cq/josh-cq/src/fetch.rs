use anyhow::Context;

use josh_core::git::spawn_git_command;
use josh_github_graphql::connection::GithubApiConnection;

use crate::api::fetch_required_checks;
use crate::models::{CandidatePr, CqActorState};

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
        let tracked = crate::layout::list_tracked_remotes_for_head(repo)
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
        let Some((owner, repo_name)) = state.resolve_owner_repo_logged(url) else {
            continue;
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
            state.upsert_candidate(CandidatePr::from_open_pr(url, pr));

            crate::admission::get_or_init_pr_admission(state, &pr.node_id, url, Some(api)).await;

            // Bring the PR's required checks in line with the current rulesets,
            // preserving already-known pass/fail results.
            if let Some(required) = &required
                && let Some(admission) = state.pr_admissions.get_mut(&pr.node_id)
            {
                crate::admission::sync_required_checks(admission, required);
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
