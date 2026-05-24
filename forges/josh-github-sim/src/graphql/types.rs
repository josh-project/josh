use std::collections::{BTreeMap, HashMap};

use url::Url;

pub struct MockRuleset {
    pub id: String,
    pub name: String,
    pub enforcement: String,
    pub include_refs: Vec<String>,
    pub exclude_refs: Vec<String>,
    pub required_checks: Vec<String>,
}

pub struct RepoState {
    pub prs: Vec<MockPr>,
    pub reviews: BTreeMap<i64, Vec<(String, String)>>,
    pub maintainers: Vec<String>,
    pub rulesets: Vec<MockRuleset>,
    pub closed_prs: Vec<String>,
    pub comments: Vec<(String, String)>,
}

pub struct GraphQLState {
    pub repos: HashMap<(String, String), RepoState>,
    pub webhook_url: Option<Url>,
    pub sim_url: Option<Url>,
}

impl GraphQLState {
    pub fn repo(&self, owner: &str, name: &str) -> Option<&RepoState> {
        self.repos.get(&(owner.to_string(), name.to_string()))
    }

    pub fn repo_mut(&mut self, owner: &str, name: &str) -> Option<&mut RepoState> {
        self.repos.get_mut(&(owner.to_string(), name.to_string()))
    }

    /// Finds a PR by node_id across all repos. Returns (owner, name, index_in_prs).
    pub fn find_pr_idx(&self, node_id: &str) -> Option<(&str, &str, usize)> {
        for ((owner, name), repo) in &self.repos {
            if let Some(idx) = repo.prs.iter().position(|p| p.node_id == node_id) {
                return Some((owner, name, idx));
            }
        }
        None
    }
}

pub struct MockPr {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub head_ref_oid: String,
    pub head_ref_name: String,
    pub base_ref_oid: String,
    pub base_ref_name: String,
}
