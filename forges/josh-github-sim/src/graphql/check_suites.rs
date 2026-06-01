use juniper::graphql_object;

use super::check_runs::CheckRunsConnection;
use super::context::Context;

/// Minimal stub — the sim doesn't track check suite/run state, so this always
/// returns an empty list. Real check-run state arrives via webhooks.
pub(crate) struct CheckSuitesConnection {
    pub(crate) nodes: Vec<CheckSuite>,
}

#[graphql_object(context = Context)]
impl CheckSuitesConnection {
    fn nodes(&self) -> &[CheckSuite] {
        &self.nodes
    }
}

pub(crate) struct CheckSuite;

#[graphql_object(context = Context)]
impl CheckSuite {
    fn check_runs(&self) -> CheckRunsConnection {
        CheckRunsConnection { nodes: vec![] }
    }
}
