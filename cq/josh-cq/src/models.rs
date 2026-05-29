use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use josh_github_changes::admission::AdmissionState;
use josh_github_graphql::operations::pull_request::OpenPr;
use josh_github_graphql::operations::repo::RequiredStatusCheck;
use josh_github_webhooks::webhook_types;

#[derive(Debug, Clone)]
pub(crate) struct CandidatePr {
    pub node_id: String,
    pub number: i64,
    pub repo_url: String,
    pub head_sha: String,
    pub base_sha: String,
    pub base_branch: String,
    pub title: String,
}

impl CandidatePr {
    /// Build a candidate from a GraphQL open-PR discovered during fetch.
    pub(crate) fn from_open_pr(repo_url: &str, pr: &OpenPr) -> Self {
        CandidatePr {
            node_id: pr.node_id.clone(),
            number: pr.number,
            repo_url: repo_url.to_string(),
            head_sha: pr.head_sha.clone(),
            base_sha: pr.base_sha.clone(),
            base_branch: pr.base_branch.clone(),
            title: pr.title.clone(),
        }
    }

    /// Build a candidate from a webhook pull-request payload.
    pub(crate) fn from_webhook_pr(repo_url: &str, pr: &webhook_types::PullRequest) -> Self {
        CandidatePr {
            node_id: pr.node_id.clone(),
            number: pr.number,
            repo_url: repo_url.to_string(),
            head_sha: pr.head.sha(),
            base_sha: pr.base.sha(),
            base_branch: pr.base.reference(),
            title: pr.title.clone(),
        }
    }
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

    /// Like `resolve_owner_repo`, but logs a warning when resolution fails.
    pub(crate) fn resolve_owner_repo_logged(&self, url: &str) -> Option<(String, String)> {
        let resolved = self.resolve_owner_repo(url);
        if resolved.is_none() {
            tracing::warn!(url = %url, "could not resolve owner/repo");
        }
        resolved
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
