use juniper::{ID, graphql_object};

use super::context::Context;
use super::repository::Repository;
use super::ruleset::{RefNameCondition, RepositoryRuleset, RulesetConditions};
use super::types::{GlobalNode, NodeKind};

pub(crate) struct Query;

#[graphql_object(context = Context)]
impl Query {
    async fn repository(owner: String, name: String, context: &Context) -> Option<Repository> {
        let key = (owner.clone(), name.clone());
        if context.repos.contains_key(&key) {
            Some(Repository { owner, name })
        } else {
            None
        }
    }

    fn node(id: ID, context: &Context) -> Option<NodeValue> {
        let global = GlobalNode::from_node_id(&id.to_string()).ok()?;
        let state = context.state.lock().unwrap();
        match global.kind {
            NodeKind::PullRequest => {
                let number = global.pr_number?;
                let repo = state.repo(&global.owner, &global.name)?;
                let pr = repo.prs.iter().find(|p| p.number == number)?;
                let id = GlobalNode::pr(&global.owner, &global.name, pr.number).to_node_id();
                Some(NodeValue::from(super::pull_request::PullRequest {
                    id,
                    number: pr.number as i32,
                    title: pr.title.clone(),
                    head_ref_oid: pr.head_ref_oid.clone(),
                    head_ref_name: pr.head_ref_name.clone(),
                    base_ref_oid: pr.base_ref_oid.clone(),
                    base_ref_name: pr.base_ref_name.clone(),
                    repo_owner: global.owner.clone(),
                    repo_name: global.name.clone(),
                }))
            }
            NodeKind::RepositoryRuleset => {
                let rs_id = global.ruleset_id.as_ref()?;
                let repo = state.repo(&global.owner, &global.name)?;
                let rs = repo.rulesets.iter().find(|r| r.id == *rs_id)?;
                let id = GlobalNode::ruleset(&global.owner, &global.name, &rs.id).to_node_id();
                Some(NodeValue::from(RepositoryRuleset {
                    id,
                    name: rs.name.clone(),
                    enforcement: rs.enforcement,
                    target: "BRANCH".to_string(),
                    conditions: RulesetConditions {
                        ref_name: RefNameCondition {
                            include: rs.include_refs.clone(),
                            exclude: rs.exclude_refs.clone(),
                        },
                    },
                    required_checks: rs.required_checks.clone(),
                }))
            }
        }
    }
}

#[juniper::graphql_interface(
    for = [
        super::pull_request::PullRequest,
        super::ruleset::RepositoryRuleset,
    ],
    context = Context
)]
#[allow(dead_code)]
pub(crate) trait Node {
    fn id(&self) -> &str;
}
