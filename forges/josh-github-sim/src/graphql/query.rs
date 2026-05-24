use juniper::{ID, graphql_object};

use super::context::Context;
use super::repository::Repository;
use super::ruleset::RepositoryRuleset;

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

    fn node(id: ID, context: &Context) -> Option<RepositoryRuleset> {
        let state = context.state.lock().unwrap();
        state
            .rulesets
            .iter()
            .find(|rs| rs.id == id.to_string())
            .map(|rs| RepositoryRuleset {
                id: rs.id.clone(),
                name: rs.name.clone(),
                enforcement: rs.enforcement.clone(),
                target: "BRANCH".to_string(),
                conditions: RulesetConditions {
                    ref_name: RefNameCondition {
                        include: rs.include_refs.clone(),
                        exclude: rs.exclude_refs.clone(),
                    },
                },
                required_checks: rs.required_checks.clone(),
            })
    }
}

use super::ruleset::{RefNameCondition, RulesetConditions};
