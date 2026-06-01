use juniper::graphql_object;

use super::context::Context;

pub(crate) struct CheckRunsConnection {
    pub(crate) nodes: Vec<CheckRun>,
}

#[graphql_object(context = Context)]
impl CheckRunsConnection {
    fn nodes(&self) -> &[CheckRun] {
        &self.nodes
    }
}

pub(crate) struct CheckRun;

#[graphql_object(context = Context)]
impl CheckRun {
    fn name(&self) -> &str {
        ""
    }

    fn conclusion(&self) -> Option<String> {
        None
    }
}
