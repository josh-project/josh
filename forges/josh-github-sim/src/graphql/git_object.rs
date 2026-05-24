use juniper::{Scalar, ScalarValue, WrongInputScalarTypeError, graphql_object};

use super::context::Context;
use super::pull_request::{PullRequest, PullRequestState};

#[derive(Clone, Debug, juniper::GraphQLScalar)]
#[graphql(parse_token(String))]
pub(crate) struct GitObjectID(pub(crate) String);

impl GitObjectID {
    fn to_output(&self) -> &str {
        &self.0
    }

    fn from_input<S: ScalarValue>(v: &Scalar<S>) -> Result<Self, WrongInputScalarTypeError<'_, S>> {
        v.try_to_string()
            .map(GitObjectID)
            .ok_or_else(|| WrongInputScalarTypeError {
                type_name: arcstr::literal!("String"),
                input: &**v,
            })
    }
}

pub(crate) struct GitObject {
    pub(crate) oid: String,
    pub(crate) associated_prs_nodes: Vec<PullRequest>,
}

#[graphql_object(context = Context, name = "Commit")]
impl GitObject {
    fn oid(&self) -> &str {
        &self.oid
    }

    fn associated_pull_requests(
        &self,
        _first: i32,
        _states: Option<Vec<PullRequestState>>,
    ) -> AssociatedPullRequestConnection {
        AssociatedPullRequestConnection {
            nodes: self.associated_prs_nodes.clone(),
        }
    }
}

struct AssociatedPullRequestConnection {
    nodes: Vec<PullRequest>,
}

#[graphql_object(context = Context)]
impl AssociatedPullRequestConnection {
    fn nodes(&self) -> &[PullRequest] {
        &self.nodes
    }
}
