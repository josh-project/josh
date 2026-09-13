//! Admission condition evaluation for pull requests.
//!
//! The remote-scoped inputs (who counts as a maintainer, which checks the
//! repository rulesets require) are fetched once per sync and cached in the
//! `gh_admission/` namespace of the changes ref as [`AdmissionData`]. The
//! per-PR inputs live in `PrData` (`reviews`, `checks`). Evaluation is a pure
//! function of the two: [`evaluate`].

use std::collections::BTreeMap;

use josh_changes::ChangesRef;
use josh_core::cache::Transaction;
use josh_github_graphql::operations::pull_request::PrData;
use josh_github_graphql::operations::repo::RequiredStatusCheck;
use josh_github_webhooks::webhook_types::PullRequestReviewState;
use serde::{Deserialize, Serialize};

use crate::layout::{GithubChangesRefData, GITHUB_ADMISSION_PATH};

/// How long a cached `AdmissionData` is reused before refetching: one week.
/// Maintainers and rulesets change rarely, and every fetch is several
/// repository-level GraphQL calls.
pub const ADMISSION_TTL_SECS: u64 = 7 * 24 * 3600;

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
}

impl AdmissionData {
    pub fn is_fresh(&self, now: u64, ttl_secs: u64) -> bool {
        now.saturating_sub(self.fetched_at) <= ttl_secs
    }
}

/// Result of evaluating admission conditions for one PR. Computed on read,
/// never persisted.
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

/// Evaluate admission conditions for a PR against the remote's admission
/// data. Admissible means: at least one maintainer approval, no maintainer
/// requesting changes (dismissed reviews and non-maintainer reviews are
/// ignored), and every required check present and passed.
pub fn evaluate(pr: &PrData, data: &AdmissionData) -> AdmissionStatus {
    let mut approved_by = Vec::new();
    let mut changes_requested_by = Vec::new();

    for (login, state) in &pr.reviews {
        if !data.maintainers.contains_key(login) {
            continue;
        }
        match state {
            PullRequestReviewState::Approved => approved_by.push(login.clone()),
            PullRequestReviewState::ChangesRequested => changes_requested_by.push(login.clone()),
            _ => {}
        }
    }

    // Check runs are matched by context alone: `PrData.checks` is keyed by
    // check-run name, so the ruleset's integration_id cannot be consulted.
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

    let admissible =
        !approved_by.is_empty() && changes_requested_by.is_empty() && unmet_checks.is_empty();

    AdmissionStatus {
        admissible,
        approved_by,
        changes_requested_by,
        unmet_checks,
    }
}

/// Store the remote's admission data: a sparse `GithubChangesRefData`
/// carrying only the `gh_admission/` subtree, merged into the ref.
pub fn store_admission_data(
    transaction: &Transaction,
    data: &AdmissionData,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let sparse = GithubChangesRefData {
        gh_admission: data.clone(),
        ..Default::default()
    };
    josh_changes::write_filtered(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_ADMISSION_PATH),
        &sparse,
        None,
        None,
    )?;
    Ok(())
}

/// Read the cached admission data, if present. Fails when the stored tree
/// does not decode (`sync --clean` rebuilds the ref).
pub fn read_admission_data(
    transaction: &Transaction,
    scope: &ChangesRef,
) -> anyhow::Result<Option<AdmissionData>> {
    let Some(data) = josh_changes::read_filtered::<GithubChangesRefData>(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_ADMISSION_PATH),
    )?
    else {
        return Ok(None);
    };
    let data = data.gh_admission;
    // An absent subtree decodes to the default struct; fetched_at == 0 is
    // the "never fetched" sentinel.
    Ok((data.fetched_at > 0).then_some(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use josh_github_graphql::operations::get_commit_check_runs::CheckState;

    fn pr_data(
        reviews: &[(&str, PullRequestReviewState)],
        checks: &[(&str, CheckState)],
    ) -> PrData {
        PrData {
            title: String::new(),
            body: None,
            number: 1,
            url: String::new(),
            state: "Open".to_string(),
            is_draft: false,
            author: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            merged: false,
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
        }
    }

    use PullRequestReviewState::{Approved, ChangesRequested, Commented, Dismissed};

    #[test]
    fn all_green_is_admissible() {
        let pr = pr_data(
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
        let pr = pr_data(&[("alice", Commented)], &[("build", CheckState::Success)]);
        let data = admission(&["alice"], &["build"]);
        assert!(!evaluate(&pr, &data).admissible);
    }

    #[test]
    fn non_maintainer_reviews_are_ignored() {
        let pr = pr_data(&[("bob", Approved)], &[("build", CheckState::Success)]);
        let data = admission(&["alice"], &["build"]);
        let status = evaluate(&pr, &data);
        assert!(!status.admissible);
        assert!(status.approved_by.is_empty());
    }

    #[test]
    fn changes_requested_blocks() {
        let pr = pr_data(
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
        let pr = pr_data(
            &[("alice", Approved), ("bob", Dismissed)],
            &[("build", CheckState::Success)],
        );
        let data = admission(&["alice", "bob"], &["build"]);
        assert!(evaluate(&pr, &data).admissible);
    }

    #[test]
    fn missing_failing_and_pending_checks_block() {
        let data = admission(&["alice"], &["build", "test", "lint"]);

        let pr = pr_data(&[("alice", Approved)], &[("build", CheckState::Success)]);
        let status = evaluate(&pr, &data);
        assert!(!status.admissible);
        assert_eq!(status.unmet_checks.len(), 2);

        let pr = pr_data(
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
        let pr = pr_data(&[("alice", Approved)], &[]);
        let data = admission(&["alice"], &[]);
        assert!(evaluate(&pr, &data).admissible);

        let pr = pr_data(&[], &[]);
        assert!(!evaluate(&pr, &data).admissible);
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
