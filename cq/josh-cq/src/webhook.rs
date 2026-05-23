use anyhow::Context;

use josh_github_graphql::connection::GithubApiConnection;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;

use crate::admission::{process_check_run, process_pr_review};
use crate::fetch::lookup_open_prs_by_sha;
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

pub(crate) fn handle_webhook(
    payload: &WebhookPayload,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let clone_url = &webhook_repository(payload).clone_url;

    let head_tree = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?
        .tree()
        .context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let tracked = link_files
        .iter()
        .any(|(_, filter)| filter.get_meta("remote").as_deref() == Some(clone_url.as_str()));

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
                    state.upsert_candidate(CandidatePr {
                        node_id: pr.node_id.clone(),
                        number: pr.number,
                        repo_url: clone_url.clone(),
                        head_sha: pr.head.sha(),
                        head_branch: pr.head.reference(),
                        base_sha: pr.base.sha(),
                        base_branch: pr.base.reference(),
                        title: pr.title.clone(),
                    });
                    state.get_or_init_pr_admission(&pr.node_id, clone_url, api);
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
                if candidate.repo_url == *clone_url && candidate.base_branch == *pushed_ref {
                    candidate.base_sha = e.after.clone();
                }
            }
        }

        WebhookPayload::PullRequestReview(e) => {
            process_pr_review(state, &e.pull_request.node_id, e, clone_url, api);
        }

        WebhookPayload::CheckRun(e) => {
            let pr_ids = lookup_open_prs_by_sha(api, clone_url, &e.check_run.head_sha, state);
            for pr_id in pr_ids {
                process_check_run(state, &pr_id, e, clone_url, api);
            }
        }

        WebhookPayload::Ping(_)
        | WebhookPayload::WorkflowJob(_)
        | WebhookPayload::WorkflowRun(_) => {}
    }

    Ok(())
}
