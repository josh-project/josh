use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use josh_github_changes::admission::AdmissionState;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;

use crate::fetch::{fetch_maintainers, fetch_required_checks};
use crate::types::GH_TOKEN_ENV;

#[derive(Debug, Clone)]
pub(crate) struct CandidatePr {
    pub node_id: String,
    pub number: i64,
    pub repo_url: String,
    pub head_sha: String,
    /// Kept for future use (e.g., logging / status display).
    #[allow(dead_code)]
    pub head_branch: String,
    pub base_sha: String,
    pub base_branch: String,
    pub title: String,
}

#[derive(Default)]
pub(crate) struct CqActorState {
    pub admission: BTreeMap<String, BTreeSet<RequiredStatusCheck>>,
    pub pr_admissions: BTreeMap<String, AdmissionState>,
    pub candidates: BTreeMap<String, CandidatePr>,
    /// Maps arbitrary clone URLs (e.g. 127.0.0.1 for tests) to (owner, name) pairs.
    pub url_owner_map: HashMap<String, (String, String)>,
    /// PRs that have been closed via webhook — prevents re-discovery in fetch.
    pub closed_prs: HashSet<String>,
}

impl CqActorState {
    /// Try `josh_github_changes::repo::parse_owner_repo`, fall back to an explicit map.
    pub(crate) fn resolve_owner_repo(&self, url: &str) -> Option<(String, String)> {
        match josh_github_changes::repo::parse_owner_repo(url) {
            Ok(pair) => Some(pair),
            Err(_) => self.url_owner_map.get(url).cloned(),
        }
    }

    pub fn get_or_fetch_admission(
        &mut self,
        clone_url: &str,
        api: Option<&GithubApiConnection>,
    ) -> Option<BTreeSet<RequiredStatusCheck>> {
        if let Some(checks) = self.admission.get(clone_url) {
            return Some(checks.clone());
        }

        let Some(api) = api else {
            tracing::warn!(
                url = %clone_url,
                "skipping admission populate: {} not set",
                GH_TOKEN_ENV
            );
            return None;
        };

        let (owner, name) = match self.resolve_owner_repo(clone_url) {
            Some(parts) => parts,
            None => {
                tracing::warn!(url = %clone_url, "could not resolve owner/repo");
                return None;
            }
        };

        match tokio::runtime::Handle::current().block_on(fetch_required_checks(api, &owner, &name))
        {
            Ok(checks) => {
                tracing::info!(
                    url = %clone_url,
                    count = checks.len(),
                    "populated admission entry"
                );
                self.admission.insert(clone_url.to_string(), checks.clone());
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

    pub fn get_or_init_pr_admission(
        &mut self,
        pr_node_id: &str,
        clone_url: &str,
        api: Option<&GithubApiConnection>,
    ) -> Option<&mut AdmissionState> {
        if !self.pr_admissions.contains_key(pr_node_id) {
            let required = self.get_or_fetch_admission(clone_url, api)?;
            let maintainers = fetch_maintainers(clone_url, api, self);
            let state = AdmissionState {
                required_checks: required.into_iter().map(|c| (c, false)).collect(),
                maintainer_reviews: BTreeMap::new(),
                maintainers: maintainers.into_iter().collect(),
            };
            tracing::info!(
                pr = %pr_node_id,
                url = %clone_url,
                "initialized pr_admission entry"
            );
            self.pr_admissions.insert(pr_node_id.to_string(), state);
        }
        self.pr_admissions.get_mut(pr_node_id)
    }

    pub fn upsert_candidate(&mut self, pr: CandidatePr) {
        self.candidates.insert(pr.node_id.clone(), pr);
    }

    pub fn remove_candidate(&mut self, pr_node_id: &str) {
        self.candidates.remove(pr_node_id);
        self.pr_admissions.remove(pr_node_id);
        self.closed_prs.remove(pr_node_id);
    }

    pub fn get_candidate(&self, pr_node_id: &str) -> Option<&CandidatePr> {
        self.candidates.get(pr_node_id)
    }
}
