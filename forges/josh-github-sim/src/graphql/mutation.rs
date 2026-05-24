use juniper::{ID, graphql_object};

use super::context::Context;

#[derive(juniper::GraphQLInputObject)]
pub(crate) struct AddCommentInput {
    pub(crate) subject_id: ID,
    pub(crate) body: String,
}

#[derive(juniper::GraphQLInputObject)]
pub(crate) struct ClosePullRequestInput {
    pub(crate) pull_request_id: ID,
}

pub(crate) struct Mutation;

#[graphql_object(context = Context)]
impl Mutation {
    fn close_pull_request(
        input: ClosePullRequestInput,
        context: &Context,
    ) -> ClosePullRequestPayload {
        let pull_request_node_id = input.pull_request_id.to_string();
        let mut state = context.state.lock().unwrap();
        if let Some((owner, name, idx)) = state.find_pr_idx(&pull_request_node_id) {
            let key = (owner.to_string(), name.to_string());
            if let Some(repo) = state.repos.get_mut(&key) {
                repo.closed_prs.push(pull_request_node_id.clone());
                repo.prs.remove(idx);
            }
        }
        ClosePullRequestPayload {
            pull_request: ClosePullRequestResult {
                id: pull_request_node_id,
            },
        }
    }

    fn add_comment(input: AddCommentInput, context: &Context) -> AddCommentPayload {
        let subject_id = input.subject_id.to_string();
        let body = input.body;
        let mut state = context.state.lock().unwrap();
        // Find which repo this subject (PR) belongs to
        let key = state
            .find_pr_idx(&subject_id)
            .map(|(owner, name, _)| (owner.to_string(), name.to_string()));
        if let Some(key) = key {
            if let Some(repo) = state.repos.get_mut(&key) {
                repo.comments.push((subject_id, body));
            }
        }
        AddCommentPayload {
            client_mutation_id: None,
        }
    }
}

struct ClosePullRequestPayload {
    pull_request: ClosePullRequestResult,
}

#[graphql_object(context = Context)]
impl ClosePullRequestPayload {
    fn pull_request(&self) -> &ClosePullRequestResult {
        &self.pull_request
    }
}

struct ClosePullRequestResult {
    id: String,
}

#[graphql_object(context = Context)]
impl ClosePullRequestResult {
    fn id(&self) -> &str {
        &self.id
    }
}

struct AddCommentPayload {
    client_mutation_id: Option<String>,
}

#[graphql_object(context = Context)]
impl AddCommentPayload {
    fn client_mutation_id(&self) -> Option<&str> {
        self.client_mutation_id.as_deref()
    }
}
