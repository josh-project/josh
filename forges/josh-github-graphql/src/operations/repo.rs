use crate::connection::GithubApiConnection;

use josh_github_codegen_graphql::{
    get_default_branch,
    get_repository_rulesets::{self, RepositoryRulesetTarget, RuleEnforcement},
    get_ruleset_required_checks::{
        self, GetRulesetRequiredChecksNode, RequiredStatusChecksInfoParameters,
    },
    GetDefaultBranch, GetRepositoryRulesets, GetRulesetRequiredChecks,
};

/// A repository ruleset with its branch conditions.
#[derive(Debug)]
pub struct RepositoryRuleset {
    pub id: String,
    pub name: String,
    pub enforcement: RuleEnforcement,
    pub target: Option<RepositoryRulesetTarget>,
    pub include_refs: Vec<String>,
    pub exclude_refs: Vec<String>,
}

/// A required status check from a ruleset.
#[derive(Debug)]
pub struct RequiredStatusCheck {
    pub context: String,
    pub integration_id: Option<i64>,
}

impl GithubApiConnection {
    /// Returns (default_branch_name, default_branch_head_oid) if available.
    pub async fn get_default_branch(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        let variables = get_default_branch::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
        };

        let response = self.make_request::<GetDefaultBranch>(variables).await?;
        let repo = match response.repository {
            Some(r) => r,
            None => return Ok(None),
        };
        let default_ref = match repo.default_branch_ref {
            Some(r) => r,
            None => return Ok(None),
        };

        let target = match default_ref.target {
            Some(t) => t,
            None => return Ok(None),
        };

        Ok(Some((default_ref.name, target.oid)))
    }

    /// Returns all rulesets for the given repository with their branch conditions.
    pub async fn get_repository_rulesets(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Vec<RepositoryRuleset>> {
        let variables = get_repository_rulesets::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
        };

        let response = self
            .make_request::<GetRepositoryRulesets>(variables)
            .await?;

        let rulesets = response
            .repository
            .and_then(|r| r.rulesets)
            .and_then(|r| r.nodes)
            .unwrap_or_default();

        Ok(rulesets
            .into_iter()
            .flatten()
            .map(|node| {
                let (include_refs, exclude_refs) = match node.conditions.ref_name {
                    Some(ref_name) => (ref_name.include, ref_name.exclude),
                    None => (vec![], vec![]),
                };
                RepositoryRuleset {
                    id: node.id,
                    name: node.name,
                    enforcement: node.enforcement,
                    target: node.target,
                    include_refs,
                    exclude_refs,
                }
            })
            .collect())
    }

    /// Returns the required status checks for the given ruleset.
    pub async fn get_ruleset_required_checks(
        &self,
        ruleset_id: &str,
    ) -> anyhow::Result<Vec<RequiredStatusCheck>> {
        let variables = get_ruleset_required_checks::Variables {
            ruleset_id: ruleset_id.to_string(),
        };

        let response = self
            .make_request::<GetRulesetRequiredChecks>(variables)
            .await?;

        let rules = match response.node {
            Some(GetRulesetRequiredChecksNode::RepositoryRuleset(ruleset)) => {
                ruleset.rules.and_then(|r| r.nodes).unwrap_or_default()
            }
            _ => return Ok(vec![]),
        };

        let checks = rules
            .into_iter()
            .flatten()
            .filter_map(|rule| match rule.parameters {
                Some(RequiredStatusChecksInfoParameters::RequiredStatusChecksParameters(
                    params,
                )) => Some(params.required_status_checks),
                _ => None,
            })
            .flatten()
            .map(|check| RequiredStatusCheck {
                context: check.context,
                integration_id: check.integration_id,
            })
            .collect();

        Ok(checks)
    }
}
