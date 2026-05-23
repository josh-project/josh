use josh_github_graphql::connection::GithubApiConnection;

use crate::models::CqActorState;
use crate::types::AdmissionRelevantEvent;

pub(crate) fn process_admission_events(
    state: &mut CqActorState,
    events: &[(String, AdmissionRelevantEvent<'_>)],
    clone_url: &str,
    api: Option<&GithubApiConnection>,
) {
    for (pr_node_id, evt) in events {
        let Some(admission) = state.get_or_init_pr_admission(pr_node_id, clone_url, api) else {
            continue;
        };
        match evt {
            AdmissionRelevantEvent::PullRequestReview(e) => {
                admission.process_pr_review_events(std::slice::from_ref(e));
            }
            AdmissionRelevantEvent::CheckRun(e) => {
                admission.process_check_run_events(std::slice::from_ref(e));
            }
        }
    }
}
