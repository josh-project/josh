use juniper::graphql_object;

use super::context::Context;
use super::query::NodeValue;
use super::types::RuleEnforcement;

#[derive(juniper::GraphQLEnum)]
pub(crate) enum RepositoryRuleType {
    RequiredStatusChecks,
}

pub(crate) struct RefNameCondition {
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
}

#[graphql_object(context = Context)]
impl RefNameCondition {
    fn include(&self) -> &[String] {
        &self.include
    }
    fn exclude(&self) -> &[String] {
        &self.exclude
    }
}

pub(crate) struct RulesetConditions {
    pub(crate) ref_name: RefNameCondition,
}

#[graphql_object(context = Context)]
impl RulesetConditions {
    fn ref_name(&self) -> &RefNameCondition {
        &self.ref_name
    }
}

pub(crate) struct RepositoryRuleset {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) enforcement: RuleEnforcement,
    pub(crate) target: String,
    pub(crate) conditions: RulesetConditions,
    pub(crate) required_checks: Vec<String>,
}

#[graphql_object(context = Context, impl = NodeValue)]
impl RepositoryRuleset {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn enforcement(&self) -> &RuleEnforcement {
        &self.enforcement
    }
    fn target(&self) -> &str {
        &self.target
    }
    fn conditions(&self) -> &RulesetConditions {
        &self.conditions
    }
    fn rules(&self, _first: i32, _type: Option<RepositoryRuleType>) -> RulesConnection {
        if self.required_checks.is_empty() {
            return RulesConnection { nodes: vec![] };
        }
        let nodes = vec![RepositoryRule {
            rule_type: "REQUIRED_STATUS_CHECKS".to_string(),
            parameters: RuleParameters {
                required_status_checks: self
                    .required_checks
                    .iter()
                    .map(|context_str| RequiredStatusCheck {
                        context: context_str.clone(),
                        integration_id: None,
                    })
                    .collect(),
                strict_required_status_checks_policy: false,
            },
        }];
        RulesConnection { nodes }
    }
}

pub(crate) struct RulesConnection {
    nodes: Vec<RepositoryRule>,
}

#[graphql_object(context = Context)]
impl RulesConnection {
    fn nodes(&self) -> &[RepositoryRule] {
        &self.nodes
    }
}

struct RepositoryRule {
    rule_type: String,
    parameters: RuleParameters,
}

#[graphql_object(context = Context)]
impl RepositoryRule {
    #[graphql(name = "type")]
    fn rule_type(&self) -> &str {
        &self.rule_type
    }
    fn parameters(&self) -> &RuleParameters {
        &self.parameters
    }
}

struct RuleParameters {
    required_status_checks: Vec<RequiredStatusCheck>,
    strict_required_status_checks_policy: bool,
}

#[graphql_object(context = Context, name = "RequiredStatusChecksParameters")]
impl RuleParameters {
    fn required_status_checks(&self) -> &[RequiredStatusCheck] {
        &self.required_status_checks
    }
    fn strict_required_status_checks_policy(&self) -> bool {
        self.strict_required_status_checks_policy
    }
}

struct RequiredStatusCheck {
    context: String,
    integration_id: Option<i32>,
}

#[graphql_object(context = Context)]
impl RequiredStatusCheck {
    fn context(&self) -> &str {
        &self.context
    }
    fn integration_id(&self) -> Option<i32> {
        self.integration_id
    }
}

pub(crate) struct RulesetConnection {
    pub(crate) nodes: Vec<RepositoryRuleset>,
}

#[graphql_object(context = Context)]
impl RulesetConnection {
    fn nodes(&self) -> &[RepositoryRuleset] {
        &self.nodes
    }
}
