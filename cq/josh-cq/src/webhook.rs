use anyhow::Context;

use josh_github_graphql::connection::GithubApiConnection;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;

use crate::admission::get_or_init_pr_admission;
use crate::api::lookup_open_prs_by_sha;
use crate::models::{CandidatePr, CqActorState};

fn webhook_repository(payload: &WebhookPayload) -> &webhook_types::Repository {
    match payload {
        WebhookPayload::Ping(e) => &e.repository,
        WebhookPayload::Push(e) => &e.repository,
        WebhookPayload::PullRequest(e) => &e.repository,
        WebhookPayload::WorkflowJob(e) => &e.repository,
        WebhookPayload::WorkflowRun(e) => &e.repository,
        WebhookPayload::CheckRun(e) => &e.repository,
        WebhookPayload::PullRequestReview(e) => &e.repository,
    }
}

pub(crate) async fn handle_webhook(
    payload: &WebhookPayload,
    transaction: josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    let clone_url = webhook_repository(payload).clone_url.clone();
    let clone_url_for_closure = clone_url.clone();

    let tracked = tokio::task::spawn_blocking(move || {
        let repo = transaction.repo();
        Ok::<_, anyhow::Error>(
            crate::layout::find_remote_by_url(repo, &clone_url_for_closure)
                .context("Failed to list tracked remotes")?
                .is_some(),
        )
    })
    .await??;

    if !tracked {
        tracing::info!(url = %clone_url, "ignoring webhook from untracked repo");
        return Ok(());
    }

    tracing::info!(url = %clone_url, "received webhook from tracked repo");

    match payload {
        WebhookPayload::PullRequest(e) => {
            let pr = &e.pull_request;
            match &e.details {
                webhook_types::PullRequestEventDetails::Opened
                | webhook_types::PullRequestEventDetails::Synchronize { .. } => {
                    state.upsert_candidate(CandidatePr::from_webhook_pr(&clone_url, pr));
                    get_or_init_pr_admission(state, &pr.node_id, &clone_url, api).await;
                }
                webhook_types::PullRequestEventDetails::Closed => {
                    state.remove_candidate(&pr.node_id);
                    state.closed_prs.insert(pr.node_id.clone());
                }
                _ => {}
            }
        }

        WebhookPayload::Push(e) => {
            let pushed_ref = &e.ref_;
            for candidate in state.candidates.values_mut() {
                if candidate.repo_url == clone_url && candidate.base_branch == *pushed_ref {
                    candidate.base_sha = e.after.clone();
                }
            }
        }

        WebhookPayload::PullRequestReview(e) => {
            if let Some(admission) =
                get_or_init_pr_admission(state, &e.pull_request.node_id, &clone_url, api).await
            {
                admission.process_pr_review_events(std::slice::from_ref(e));
            }
        }

        WebhookPayload::CheckRun(e) => {
            let pr_ids =
                lookup_open_prs_by_sha(api, &clone_url, &e.check_run.head_sha, state).await;
            for pr_id in pr_ids {
                if let Some(admission) =
                    get_or_init_pr_admission(state, &pr_id, &clone_url, api).await
                {
                    admission.process_check_run_events(std::slice::from_ref(e));
                }
            }
        }

        WebhookPayload::Ping(_)
        | WebhookPayload::WorkflowJob(_)
        | WebhookPayload::WorkflowRun(_) => {}
    }

    Ok(())
}
