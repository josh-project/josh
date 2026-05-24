use juniper::graphql_object;

use super::context::Context;
use super::context::{PageInfo, User};

pub(crate) struct CollaboratorEdge {
    pub(crate) permission: String,
    pub(crate) node: User,
}

#[graphql_object(context = Context, name = "RepositoryCollaboratorEdge")]
impl CollaboratorEdge {
    fn permission(&self) -> &str {
        &self.permission
    }
    fn node(&self) -> &User {
        &self.node
    }
}

pub(crate) struct CollaboratorConnection {
    pub(crate) edges: Vec<CollaboratorEdge>,
}

#[graphql_object(context = Context, name = "RepositoryCollaboratorConnection")]
impl CollaboratorConnection {
    fn edges(&self) -> &[CollaboratorEdge] {
        &self.edges
    }
    fn page_info(&self) -> PageInfo {
        PageInfo {
            has_next_page: false,
            end_cursor: None,
        }
    }
}
