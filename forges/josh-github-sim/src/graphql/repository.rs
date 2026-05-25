use juniper::graphql_object;

use super::collaborator::{CollaboratorConnection, CollaboratorEdge};
use super::context::Context;
use super::context::User;
use super::git_object::{GitObject, GitObjectID};
use super::pull_request::{PullRequest, PullRequestConnection, PullRequestState};
use super::ruleset::{RefNameCondition, RepositoryRuleset, RulesetConditions, RulesetConnection};

pub(crate) struct Repository {
    pub(crate) owner: String,
    pub(crate) name: String,
}

#[graphql_object(context = Context)]
impl Repository {
    fn name_with_owner(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    fn default_branch_ref(&self) -> DefaultBranchRef {
        DefaultBranchRef
    }

    fn pull_requests(
        &self,
        first: i32,
        _after: Option<String>,
        states: Option<Vec<PullRequestState>>,
        context: &Context,
    ) -> PullRequestConnection {
        let state = context.state.lock().unwrap();
        let all_prs: Vec<&MockPr> = state
            .repo(&self.owner, &self.name)
            .map(|repo| {
                repo.prs
                    .iter()
                    .filter(|_pr| {
                        if let Some(ref states) = states {
                            states.iter().any(|s| matches!(s, PullRequestState::OPEN))
                        } else {
                            true
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let total_count = all_prs.len() as i32;
        let nodes: Vec<PullRequest> = all_prs
            .into_iter()
            .take(first as usize)
            .map(|pr| PullRequest {
                id: pr.node_id.clone(),
                number: pr.number as i32,
                title: pr.title.clone(),
                head_ref_oid: pr.head_ref_oid.clone(),
                head_ref_name: pr.head_ref_name.clone(),
                base_ref_oid: pr.base_ref_oid.clone(),
                base_ref_name: pr.base_ref_name.clone(),
                repo_owner: self.owner.clone(),
                repo_name: self.name.clone(),
            })
            .collect();
        PullRequestConnection { nodes, total_count }
    }

    fn pull_request(&self, number: i32, context: &Context) -> Option<PullRequest> {
        let state = context.state.lock().unwrap();
        state
            .repo(&self.owner, &self.name)
            .and_then(|repo| repo.prs.iter().find(|pr| pr.number == number as i64))
            .map(|pr| PullRequest {
                id: pr.node_id.clone(),
                number: pr.number as i32,
                title: pr.title.clone(),
                head_ref_oid: pr.head_ref_oid.clone(),
                head_ref_name: pr.head_ref_name.clone(),
                base_ref_oid: pr.base_ref_oid.clone(),
                base_ref_name: pr.base_ref_name.clone(),
                repo_owner: self.owner.clone(),
                repo_name: self.name.clone(),
            })
    }

    fn collaborators(
        &self,
        first: i32,
        _after: Option<String>,
        context: &Context,
    ) -> CollaboratorConnection {
        let state = context.state.lock().unwrap();
        let edges: Vec<CollaboratorEdge> = state
            .repo(&self.owner, &self.name)
            .map(|repo| {
                repo.maintainers
                    .iter()
                    .take(first as usize)
                    .map(|login| CollaboratorEdge {
                        permission: "WRITE".to_string(),
                        node: User {
                            login: login.clone(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        CollaboratorConnection { edges }
    }

    fn rulesets(
        &self,
        first: i32,
        _include_parents: Option<bool>,
        context: &Context,
    ) -> RulesetConnection {
        let state = context.state.lock().unwrap();
        let nodes: Vec<RepositoryRuleset> = state
            .repo(&self.owner, &self.name)
            .map(|repo| repo.rulesets.iter())
            .into_iter()
            .flatten()
            .take(first as usize)
            .map(|rs| RepositoryRuleset {
                id: rs.id.clone(),
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
            })
            .collect();
        RulesetConnection { nodes }
    }

    fn object(&self, oid: GitObjectID, context: &Context) -> Option<GitObject> {
        let oid = oid.0;
        let state = context.state.lock().unwrap();
        let repo = state.repo(&self.owner, &self.name)?;
        let has_matching_pr = repo
            .prs
            .iter()
            .any(|pr| pr.head_ref_oid == oid || pr.base_ref_oid == oid);
        if has_matching_pr {
            let oid_clone = oid.clone();
            Some(GitObject {
                oid,
                associated_prs_nodes: repo
                    .prs
                    .iter()
                    .filter(|pr| pr.head_ref_oid == oid_clone)
                    .map(|pr| PullRequest {
                        id: pr.node_id.clone(),
                        number: pr.number as i32,
                        title: pr.title.clone(),
                        head_ref_oid: pr.head_ref_oid.clone(),
                        head_ref_name: pr.head_ref_name.clone(),
                        base_ref_oid: pr.base_ref_oid.clone(),
                        base_ref_name: pr.base_ref_name.clone(),
                        repo_owner: self.owner.clone(),
                        repo_name: self.name.clone(),
                    })
                    .collect(),
            })
        } else {
            None
        }
    }
}

struct DefaultBranchRef;

#[graphql_object(context = Context)]
impl DefaultBranchRef {
    fn name(&self) -> &str {
        "main"
    }
}

use super::types::MockPr;
