use std::collections::{BTreeMap, BTreeSet};

use josh_github_changes::admission::AdmissionState;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;

use crate::api::{fetch_maintainers, fetch_required_checks};
use crate::models::CqActorState;

pub(crate) async fn get_or_fetch_admission(
    state: &mut CqActorState,
    clone_url: &str,
    api: &GithubApiConnection,
) -> Option<BTreeSet<RequiredStatusCheck>> {
    if let Some(checks) = state.admission.get(clone_url) {
        return Some(checks.clone());
    }

    let (owner, name) = state.resolve_owner_repo(clone_url)?;

    match fetch_required_checks(api, &owner, &name).await {
        Ok(checks) => {
            tracing::info!(
                url = %clone_url,
                count = checks.len(),
                "populated admission entry"
            );
            state
                .admission
                .insert(clone_url.to_string(), checks.clone());
            Some(checks)
        }
        Err(e) => {
            tracing::error!(
                url = %clone_url,
                error = ?e,
                "failed to fetch required checks; will retry on next webhook"
            );
            None
        }
    }
}

pub(crate) async fn get_or_init_pr_admission<'a>(
    state: &'a mut CqActorState,
    pr_node_id: &str,
    clone_url: &str,
    api: &GithubApiConnection,
) -> Option<&'a mut AdmissionState> {
    if !state.pr_admissions.contains_key(pr_node_id) {
        let required = get_or_fetch_admission(state, clone_url, api).await?;
        let maintainers = fetch_maintainers(clone_url, api, state).await;
        let admission = AdmissionState {
            required_checks: required.into_iter().map(|c| (c, false)).collect(),
            maintainer_reviews: BTreeMap::new(),
            maintainers: maintainers.into_iter().collect(),
        };
        tracing::info!(
            pr = %pr_node_id,
            url = %clone_url,
            "initialized pr_admission entry"
        );
        state
            .pr_admissions
            .insert(pr_node_id.to_string(), admission);
    }
    state.pr_admissions.get_mut(pr_node_id)
}

/// Bring a PR's required checks in line with `required`, preserving known results.
pub(crate) fn sync_required_checks(
    admission: &mut AdmissionState,
    required: &BTreeSet<RequiredStatusCheck>,
) {
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
