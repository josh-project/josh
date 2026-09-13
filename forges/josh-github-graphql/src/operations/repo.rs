use crate::connection::GithubApiConnection;

use josh_github_codegen_graphql::{
    get_branch_protection_rules, get_default_branch,
    get_repository_rulesets::{self, RepositoryRulesetTarget, RuleEnforcement},
    get_ruleset_required_checks::{
        self, GetRulesetRequiredChecksNode, RequiredStatusChecksInfoParameters,
    },
    GetBranchProtectionRules, GetDefaultBranch, GetRepositoryRulesets, GetRulesetRequiredChecks,
};
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequiredStatusCheck {
    pub context: String,
    pub integration_id: Option<i64>,
}

/// A classic branch protection rule.
#[derive(Debug)]
pub struct BranchProtectionRuleInfo {
    /// fnmatch pattern on short branch names (e.g. "master", "release-*").
    pub pattern: String,
    pub required_checks: Vec<RequiredStatusCheck>,
    pub required_approvals: u32,
}

/// Repository-level admission requirements for one branch, merged from
/// rulesets and classic branch protection rules.
#[derive(Debug, Default)]
pub struct AdmissionRequirements {
    pub required_checks: Vec<RequiredStatusCheck>,
    pub required_approvals: u32,
}

/// fnmatch-lite for branch protection patterns: `*` matches any (possibly
/// empty) character sequence, everything else is literal.
fn branch_pattern_matches(pattern: &str, branch: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == branch;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = branch;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            let Some(r) = rest.strip_prefix(part) else {
                return false;
            };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(pos) = rest.find(part) else {
                return false;
            };
            rest = &rest[pos + part.len()..];
        }
    }
    true
}

impl RepositoryRuleset {
    /// Whether the ruleset's branch conditions cover `branch` (a short
    /// branch name). Supports "~ALL", exact "refs/heads/<branch>" and
    /// trailing-`*` prefixes; "~DEFAULT_BRANCH" is not resolved and never
    /// matches.
    pub fn targets_branch(&self, branch: &str) -> bool {
        let refname = format!("refs/heads/{}", branch);
        let matches = |pattern: &str| {
            pattern == "~ALL"
                || pattern == refname
                || pattern
                    .strip_suffix('*')
                    .is_some_and(|prefix| refname.starts_with(prefix))
        };
        let included = self.include_refs.is_empty() || self.include_refs.iter().any(|p| matches(p));
        let excluded = self.exclude_refs.iter().any(|p| matches(p));
        included && !excluded
    }
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

    /// Required status checks from all active rulesets targeting `branch`
    /// (a short branch name).
    pub async fn get_required_checks(
        &self,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> anyhow::Result<Vec<RequiredStatusCheck>> {
        let mut checks = Vec::new();
        for ruleset in self.get_repository_rulesets(owner, name).await? {
            if !matches!(ruleset.enforcement, RuleEnforcement::Active)
                || !ruleset.targets_branch(branch)
            {
                continue;
            }
            checks.extend(self.get_ruleset_required_checks(&ruleset.id).await?);
        }
        Ok(checks)
    }

    /// Classic branch protection rules, with their required status checks.
    pub async fn get_branch_protection_rules(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Vec<BranchProtectionRuleInfo>> {
        let variables = get_branch_protection_rules::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
        };

        let response = self
            .make_request::<GetBranchProtectionRules>(variables)
            .await?;

        let rules = response
            .repository
            .map(|r| r.branch_protection_rules)
            .and_then(|r| r.nodes)
            .unwrap_or_default();

        Ok(rules
            .into_iter()
            .flatten()
            .map(|node| BranchProtectionRuleInfo {
                pattern: node.pattern,
                required_checks: node
                    .required_status_checks
                    .unwrap_or_default()
                    .into_iter()
                    .map(|check| RequiredStatusCheck {
                        context: check.context,
                        integration_id: check
                            .app
                            .and_then(|app| app.database_id)
                            .map(|id| id as i64),
                    })
                    .collect(),
                required_approvals: node.required_approving_review_count.unwrap_or(0).max(0) as u32,
            })
            .collect())
    }

    /// Admission requirements for `branch` (a short branch name), merged
    /// from active rulesets and matching classic branch protection rules.
    pub async fn get_admission_requirements(
        &self,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> anyhow::Result<AdmissionRequirements> {
        let mut requirements = AdmissionRequirements {
            required_checks: self.get_required_checks(owner, name, branch).await?,
            required_approvals: 0,
        };
        for rule in self.get_branch_protection_rules(owner, name).await? {
            if !branch_pattern_matches(&rule.pattern, branch) {
                continue;
            }
            requirements.required_checks.extend(rule.required_checks);
            requirements.required_approvals =
                requirements.required_approvals.max(rule.required_approvals);
        }
        Ok(requirements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_branch_patterns() {
        assert!(branch_pattern_matches("master", "master"));
        assert!(!branch_pattern_matches("master", "main"));
        assert!(!branch_pattern_matches("master", "master-2"));
    }

    #[test]
    fn glob_branch_patterns() {
        assert!(branch_pattern_matches("release-*", "release-1.0"));
        assert!(!branch_pattern_matches("release-*", "release/1.0.2-alpha"));
        assert!(branch_pattern_matches("release/*", "release/1.0.2-alpha"));
        assert!(branch_pattern_matches("*-hotfix", "august-hotfix"));
        assert!(branch_pattern_matches("rel*", "release"));
        assert!(branch_pattern_matches("*", "anything"));
        assert!(branch_pattern_matches("a*b*c", "axbyc"));
        assert!(!branch_pattern_matches("a*b*c", "acb"));
        assert!(!branch_pattern_matches("release-*", "release"));
    }
}
