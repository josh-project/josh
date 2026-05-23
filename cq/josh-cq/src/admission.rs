use josh_github_graphql::connection::GithubApiConnection;
use josh_github_webhooks::webhook_types;

use crate::models::CqActorState;

pub(crate) async fn process_pr_review(
    state: &mut CqActorState,
    pr_node_id: &str,
    event: &webhook_types::PullRequestReviewEvent,
    clone_url: &str,
    api: Option<&GithubApiConnection>,
) {
    if let Some(admission) = state
        .get_or_init_pr_admission(pr_node_id, clone_url, api)
        .await
    {
        admission.process_pr_review_events(std::slice::from_ref(event));
    }
}

pub(crate) async fn process_check_run(
    state: &mut CqActorState,
    pr_node_id: &str,
    event: &webhook_types::CheckRunEvent,
    clone_url: &str,
    api: Option<&GithubApiConnection>,
) {
    if let Some(admission) = state
        .get_or_init_pr_admission(pr_node_id, clone_url, api)
        .await
    {
        admission.process_check_run_events(std::slice::from_ref(event));
    }
}
