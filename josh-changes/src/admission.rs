//! Admission condition evaluation for changes.
//!
//! The remote-scoped inputs (who counts as a maintainer, which checks the
//! target branch requires, how many approvals a merge needs) are synchronized
//! per forge into the changes ref as [`AdmissionData`] -- GitHub sync stores
//! it in the `gh_admission/` namespace, test-forge sync in `test_admission/`.
//! The per-change inputs live in `ChangeData` (`reviews`, `checks`).
//! Evaluation is a pure function of the two: [`evaluate`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::change_data::{ChangeData, RequiredStatusCheck, ReviewState};

/// Remote-scoped admission inputs, cached on the changes ref.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AdmissionData {
    /// Unix seconds of the fetch, for the TTL.
    #[serde(default)]
    pub fetched_at: u64,
    /// Maintainer logins (write/maintain/admin collaborators). Unit values:
    /// the git-tree format has no sequences, so this is a set of entries.
    #[serde(default)]
    pub maintainers: BTreeMap<String, ()>,
    /// Required status checks from the repository rulesets, keyed by context.
    #[serde(default)]
    pub required_checks: BTreeMap<String, RequiredStatusCheck>,
    /// Required approving reviews (from classic branch protection; ruleset
    /// pull-request rules are not read yet). Zero means "unspecified" — the
    /// evaluation floor is one maintainer approval regardless.
    #[serde(default)]
    pub required_approvals: u32,
}

impl AdmissionData {
    pub fn is_fresh(&self, now: u64, ttl_secs: u64) -> bool {
        now.saturating_sub(self.fetched_at) <= ttl_secs
    }
}

/// Result of evaluating admission conditions for one change. Computed on
/// read, never persisted.
#[derive(Debug, Default)]
pub struct AdmissionStatus {
    pub admissible: bool,
    /// Maintainers whose latest review is an approval.
    pub approved_by: Vec<String>,
    /// Maintainers whose latest review requests changes.
    pub changes_requested_by: Vec<String>,
    /// Required checks that are missing or not passed.
    pub unmet_checks: Vec<RequiredStatusCheck>,
}

/// Evaluate admission conditions for a change against the remote's admission
/// data. Admissible means: enough maintainer approvals (at least one, more
/// when branch protection requires it), no maintainer requesting changes
/// (dismissed reviews and non-maintainer reviews are ignored), and every
/// required check present and passed.
pub fn evaluate(pr: &ChangeData, data: &AdmissionData) -> AdmissionStatus {
    let mut approved_by = Vec::new();
    let mut changes_requested_by = Vec::new();

    for (login, state) in &pr.reviews {
        if !data.maintainers.contains_key(login) {
            continue;
        }
        match state {
            ReviewState::Approved => approved_by.push(login.clone()),
            ReviewState::ChangesRequested => changes_requested_by.push(login.clone()),
            _ => {}
        }
    }

    // Check runs are matched by context alone: `ChangeData.checks` is keyed
    // by check-run name, so the ruleset's integration_id cannot be
    // consulted.
    let unmet_checks = data
        .required_checks
        .values()
        .filter(|required| {
            pr.checks
                .get(&required.context)
                .is_none_or(|state| !state.passed())
        })
        .cloned()
        .collect::<Vec<_>>();

    let required_approvals = (data.required_approvals as usize).max(1);
    let admissible = approved_by.len() >= required_approvals
        && changes_requested_by.is_empty()
        && unmet_checks.is_empty();

    AdmissionStatus {
        admissible,
        approved_by,
        changes_requested_by,
        unmet_checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_data::CheckState;

    fn change_data(reviews: &[(&str, ReviewState)], checks: &[(&str, CheckState)]) -> ChangeData {
        ChangeData {
            title: String::new(),
            body: None,
            number: None,
            url: None,
            state: "Open".to_string(),
            is_draft: None,
            author: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            merged: None,
            merged_at: None,
            merged_by: None,
            additions: 0,
            deletions: 0,
            changed_files: 0,
            base_ref_name: String::new(),
            head_ref_name: String::new(),
            reviews: reviews
                .iter()
                .map(|(login, state)| (login.to_string(), state.clone()))
                .collect(),
            checks: checks
                .iter()
                .map(|(name, state)| (name.to_string(), *state))
                .collect(),
            labels: Vec::new(),
            comments: Vec::new(),
        }
    }

    fn admission(maintainers: &[&str], required: &[&str]) -> AdmissionData {
        AdmissionData {
            fetched_at: 0,
            maintainers: maintainers
                .iter()
                .map(|login| (login.to_string(), ()))
                .collect(),
            required_checks: required
                .iter()
                .map(|context| {
                    (
                        context.to_string(),
                        RequiredStatusCheck {
                            context: context.to_string(),
                            integration_id: None,
                        },
                    )
                })
                .collect(),
            required_approvals: 0,
        }
    }

    use ReviewState::{Approved, ChangesRequested, Commented, Dismissed};

    #[test]
    fn all_green_is_admissible() {
        let pr = change_data(
            &[("alice", Approved)],
            &[
                ("build", CheckState::Success),
                ("test", CheckState::Success),
            ],
        );
        let data = admission(&["alice"], &["build", "test"]);
        let status = evaluate(&pr, &data);
        assert!(status.admissible);
        assert_eq!(status.approved_by, vec!["alice".to_string()]);
        assert!(status.unmet_checks.is_empty());
    }

    #[test]
    fn approval_is_required() {
        let pr = change_data(&[("alice", Commented)], &[("build", CheckState::Success)]);
        let data = admission(&["alice"], &["build"]);
        assert!(!evaluate(&pr, &data).admissible);
    }

    #[test]
    fn non_maintainer_reviews_are_ignored() {
        let pr = change_data(&[("bob", Approved)], &[("build", CheckState::Success)]);
        let data = admission(&["alice"], &["build"]);
        let status = evaluate(&pr, &data);
        assert!(!status.admissible);
        assert!(status.approved_by.is_empty());
    }

    #[test]
    fn changes_requested_blocks() {
        let pr = change_data(
            &[("alice", Approved), ("bob", ChangesRequested)],
            &[("build", CheckState::Success)],
        );
        let data = admission(&["alice", "bob"], &["build"]);
        let status = evaluate(&pr, &data);
        assert!(!status.admissible);
        assert_eq!(status.changes_requested_by, vec!["bob".to_string()]);
    }

    #[test]
    fn dismissed_reviews_are_ignored() {
        let pr = change_data(
            &[("alice", Approved), ("bob", Dismissed)],
            &[("build", CheckState::Success)],
        );
        let data = admission(&["alice", "bob"], &["build"]);
        assert!(evaluate(&pr, &data).admissible);
    }

    #[test]
    fn missing_failing_and_pending_checks_block() {
        let data = admission(&["alice"], &["build", "test", "lint"]);

        let pr = change_data(&[("alice", Approved)], &[("build", CheckState::Success)]);
        let status = evaluate(&pr, &data);
        assert!(!status.admissible);
        assert_eq!(status.unmet_checks.len(), 2);

        let pr = change_data(
            &[("alice", Approved)],
            &[
                ("build", CheckState::Success),
                ("test", CheckState::Failure),
                ("lint", CheckState::Pending),
            ],
        );
        let status = evaluate(&pr, &data);
        assert!(!status.admissible);
        assert_eq!(status.unmet_checks.len(), 2);
    }

    #[test]
    fn no_required_checks_still_needs_approval() {
        let pr = change_data(&[("alice", Approved)], &[]);
        let data = admission(&["alice"], &[]);
        assert!(evaluate(&pr, &data).admissible);

        let pr = change_data(&[], &[]);
        assert!(!evaluate(&pr, &data).admissible);
    }

    #[test]
    fn required_approvals_raises_the_bar() {
        let pr = change_data(&[("alice", Approved)], &[("build", CheckState::Success)]);
        let mut data = admission(&["alice", "bob"], &["build"]);
        data.required_approvals = 2;
        assert!(!evaluate(&pr, &data).admissible);

        let pr = change_data(
            &[("alice", Approved), ("bob", Approved)],
            &[("build", CheckState::Success)],
        );
        assert!(evaluate(&pr, &data).admissible);
    }

    #[test]
    fn freshness_respects_ttl() {
        let mut data = admission(&[], &[]);
        data.fetched_at = 1000;
        assert!(data.is_fresh(1000, 3600));
        assert!(data.is_fresh(4600, 3600));
        assert!(!data.is_fresh(4601, 3600));
        assert!(data.is_fresh(500, 3600));
    }
}
