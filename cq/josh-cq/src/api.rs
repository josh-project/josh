use std::collections::BTreeSet;

use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;

use crate::models::CqActorState;

pub(crate) async fn fetch_maintainers(
    clone_url: &str,
    api: Option<&GithubApiConnection>,
    state: &CqActorState,
) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let Some((owner, name)) = state.resolve_owner_repo_logged(clone_url) else {
        return Vec::new();
    };
    match api.get_maintainers(&owner, &name).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(url = %clone_url, error = ?e, "failed to fetch maintainers");
            Vec::new()
        }
    }
}

pub(crate) async fn fetch_required_checks(
    api: &GithubApiConnection,
    owner: &str,
    name: &str,
) -> anyhow::Result<BTreeSet<RequiredStatusCheck>> {
    let rulesets = api.get_repository_rulesets(owner, name).await?;
    let mut checks = BTreeSet::new();
    for ruleset in rulesets {
        if !ruleset.is_active() {
            continue;
        }
        match api.get_ruleset_required_checks(&ruleset.id).await {
            Ok(rs_checks) => checks.extend(rs_checks),
            Err(e) => tracing::warn!(
                ruleset = %ruleset.id,
                error = ?e,
                "failed to fetch checks for ruleset; skipping"
            ),
        }
    }

    Ok(checks)
}

pub(crate) async fn lookup_open_prs_by_sha(
    api: Option<&GithubApiConnection>,
    clone_url: &str,
    sha: &str,
    state: &CqActorState,
) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let Some((owner, name)) = state.resolve_owner_repo_logged(clone_url) else {
        return Vec::new();
    };
    match api.find_open_prs_by_head_sha(&owner, &name, sha).await {
        Ok(prs) => prs.into_iter().map(|(id, _)| id).collect(),
        Err(e) => {
            tracing::warn!(url = %clone_url, sha = %sha, error = ?e, "failed to look up PRs by SHA");
            Vec::new()
        }
    }
}
