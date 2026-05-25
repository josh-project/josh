use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use url::Url;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

#[derive(Debug, Clone, Copy, juniper::GraphQLEnum)]
pub enum RuleEnforcement {
    Active,
    Disabled,
    Evaluate,
}

impl RuleEnforcement {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleEnforcement::Active => "ACTIVE",
            RuleEnforcement::Disabled => "DISABLED",
            RuleEnforcement::Evaluate => "EVALUATE",
        }
    }
}

pub struct MockRuleset {
    pub id: String,
    pub name: String,
    pub enforcement: RuleEnforcement,
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

#[derive(Debug, Clone, Copy)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

impl ReviewState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewState::Approved => "APPROVED",
            ReviewState::ChangesRequested => "CHANGES_REQUESTED",
            ReviewState::Commented => "COMMENTED",
            ReviewState::Dismissed => "DISMISSED",
        }
    }
}

/// Discriminant for [`GlobalNode`] — tells what kind of object a node ID refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    PullRequest,
    RepositoryRuleset,
}

/// Introspectable global node ID.
///
/// Encoded as base64(JSON). Unlike real GitHub, the ID is transparent: decode it
/// to see what kind of object it represents and how to look it up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalNode {
    pub kind: NodeKind,
    pub owner: String,
    pub name: String,
    /// PR number, when `kind == PullRequest`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    /// Ruleset identifier, when `kind == RepositoryRuleset`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ruleset_id: Option<String>,
}

impl GlobalNode {
    pub fn pr(owner: &str, name: &str, number: i64) -> Self {
        Self {
            kind: NodeKind::PullRequest,
            owner: owner.to_string(),
            name: name.to_string(),
            pr_number: Some(number),
            ruleset_id: None,
        }
    }

    pub fn ruleset(owner: &str, name: &str, id: &str) -> Self {
        Self {
            kind: NodeKind::RepositoryRuleset,
            owner: owner.to_string(),
            name: name.to_string(),
            pr_number: None,
            ruleset_id: Some(id.to_string()),
        }
    }

    /// Encode as a base64 JSON node ID string (e.g. for use as a GraphQL `ID`).
    pub fn to_node_id(&self) -> String {
        let json = serde_json::to_string(self).expect("GlobalNode serialization should not fail");
        BASE64.encode(json)
    }

    /// Decode from a base64 JSON node ID string.
    pub fn from_node_id(id: &str) -> anyhow::Result<Self> {
        let bytes = BASE64.decode(id)?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_node_id_roundtrips() {
        let node = GlobalNode::pr("acme", "widgets", 42);
        let encoded = node.to_node_id();
        let decoded = GlobalNode::from_node_id(&encoded).unwrap();
        assert_eq!(decoded, node);
        assert_eq!(decoded.kind, NodeKind::PullRequest);
        assert_eq!(decoded.pr_number, Some(42));
    }

    #[test]
    fn ruleset_node_id_roundtrips() {
        let node = GlobalNode::ruleset("acme", "widgets", "rs-1");
        let encoded = node.to_node_id();
        let decoded = GlobalNode::from_node_id(&encoded).unwrap();
        assert_eq!(decoded, node);
        assert_eq!(decoded.kind, NodeKind::RepositoryRuleset);
        assert_eq!(decoded.ruleset_id.as_deref(), Some("rs-1"));
    }
}
