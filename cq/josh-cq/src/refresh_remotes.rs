use josh_github_graphql::connection::GithubApiConnection;

use crate::api::fetch_required_checks;
use crate::git::{GitActor, GitActorMessage};
use crate::models::{CandidatePr, CqActorState};

pub(crate) async fn refresh_remotes(
    git: &GitActor,
    api: &GithubApiConnection,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    // Enumerate tracked remotes from the metarepo via the git actor.
    let remotes: Vec<String> = git
        .request(|reply| GitActorMessage::ListTrackedRemotes { reply })
        .await?
        .into_iter()
        .map(|(_, meta)| meta.url)
        .collect();

    if remotes.is_empty() {
        tracing::info!("no tracked remotes found");
        return Ok(());
    }

    // Fetch each remote's objects through the git actor (auth attached) so PR
    // head commits are available locally. The remote's main is derived from the
    // metarepo (see step.rs), so nothing is written back here.
    for url in &remotes {
        let fetch = git
            .request(|reply| GitActorMessage::RunGitCommand {
                args: vec!["fetch".to_string(), url.clone()],
                reply,
            })
            .await;

        if let Err(e) = fetch {
            tracing::warn!(url = %url, error = ?e, "failed to fetch remote");
        }
    }

    for url in &remotes {
        let Some((owner, repo_name)) = state.resolve_owner_repo(url) else {
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
                state.required_checks.insert(url.clone(), checks.clone());
                Some(checks)
            }
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "failed to refresh required checks");
                None
            }
        };

        for pr in &prs {
            dbg!(&pr);

            if state.closed_prs.contains(&pr.node_id) {
                continue;
            }

            state.upsert_candidate(CandidatePr::from_open_pr(url, pr));

            let admission =
                crate::admission::get_or_init_pr_admission(state, &pr.node_id, url, api).await;

            // Bring the PR's required checks in line with the current rulesets,
            // preserving already-known pass/fail results.
            if let Some(required) = &required {
                crate::admission::sync_required_checks(admission, required);
            }

            match api.get_pr_reviews(&owner, &repo_name, pr.number).await {
                Ok(reviews) => {
                    admission.apply_review_states(&reviews);
                }
                Err(e) => {
                    tracing::warn!(
                        pr = %pr.node_id,
                        error = ?e,
                        "failed to fetch PR reviews"
                    );
                }
            }

            match api
                .get_commit_check_runs(&owner, &repo_name, &pr.head_sha)
                .await
            {
                Ok(results) => {
                    dbg!(&results);
                    dbg!(&admission);

                    admission.apply_check_results(&results);
                }
                Err(e) => {
                    tracing::warn!(
                        pr = %pr.node_id,
                        sha = %pr.head_sha,
                        error = ?e,
                        "failed to fetch check run results"
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
