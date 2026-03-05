use josh_github_graphql::operations::repo::RequiredStatusCheck;
use josh_github_webhooks::webhook_types::{
    CheckRunConclusion, CheckRunEvent, CheckRunEventDetails, PullRequestReviewEvent,
    PullRequestReviewEventDetails, PullRequestReviewState,
};

pub struct AdmissionState {
    pub required_checks: std::collections::BTreeMap<RequiredStatusCheck, bool>,
    pub maintainer_reviews: std::collections::BTreeMap<String, PullRequestReviewState>,
    pub maintainers: std::collections::HashSet<String>,
}

impl AdmissionState {
    pub fn process_check_run_events(&mut self, events: &[CheckRunEvent]) {
        events
            .iter()
            .filter(|e| matches!(e.details, CheckRunEventDetails::Completed))
            .for_each(|event| {
                let succeeded = matches!(
                    event.check_run.conclusion,
                    Some(CheckRunConclusion::Success)
                );

                for (check, passed) in self.required_checks.iter_mut() {
                    if check.context == event.check_run.name {
                        *passed = succeeded;
                    }
                }
            });
    }

    pub fn process_pr_review_events(&mut self, events: &[PullRequestReviewEvent]) {
        events
            .iter()
            .filter(|event| self.maintainers.contains(&event.review.user.login))
            .for_each(|event| {
                let login = &event.review.user.login;
                match event.details {
                    PullRequestReviewEventDetails::Submitted => {
                        self.maintainer_reviews
                            .insert(login.clone(), event.review.state.clone());
                    }
                    PullRequestReviewEventDetails::Dismissed => {
                        self.maintainer_reviews.remove(login);
                    }
                    PullRequestReviewEventDetails::Edited => {}
                }
            });
    }

    pub fn admissible(&self) -> bool {
        let has_approval = self
            .maintainer_reviews
            .values()
            .any(|s| matches!(s, PullRequestReviewState::Approved));

        let no_changes_requested = self
            .maintainer_reviews
            .values()
            .all(|s| !matches!(s, PullRequestReviewState::ChangesRequested));

        has_approval && no_changes_requested && self.required_checks.values().all(|&passed| passed)
    }
}
