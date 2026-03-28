use josh_github_codegen_graphql::{
    check_runs_discover::{
        self, CheckRunsDiscoverNodes, CheckRunsDiscoverNodesOnCommitCheckSuites,
        CheckRunsDiscoverNodesOnCommitCheckSuitesNodes, _CheckSuiteInfo,
    },
    CheckRunsDiscover, Id, NodeId,
};

use crate::connection::GithubApiConnection;

impl GithubApiConnection {
    // TODO:: filter out check suites matching github app id
    pub async fn check_run_state_discover(
        &self,
        node_ids: Vec<NodeId>,
    ) -> anyhow::Result<Vec<(NodeId, Vec<(NodeId, Id, usize)>)>> {
        let variables = check_runs_discover::Variables {
            node_ids: node_ids.clone(),
        };

        let response = self.make_request::<CheckRunsDiscover>(variables).await?;
        let check_suites = response
            .nodes
            .into_iter()
            .zip(node_ids)
            .filter_map(|(node, id)| match node {
                Some(CheckRunsDiscoverNodes::Commit(commit)) => match commit.check_suites {
                    Some(CheckRunsDiscoverNodesOnCommitCheckSuites {
                        nodes: Some(check_suites),
                    }) => Some((id, check_suites)),
                    _ => None,
                },
                _ => None,
            });

        let result = check_suites
            .into_iter()
            .map(|(id, check_suites)| {
                let check_runs = check_suites
                    .into_iter()
                    .flatten()
                    .filter_map(|check_suite| match check_suite {
                        CheckRunsDiscoverNodesOnCommitCheckSuitesNodes {
                            check_suite_info: _CheckSuiteInfo { id, app: Some(app) },
                            check_runs: Some(check_runs),
                        } => Some((id, app.id, check_runs.total_count as usize)),
                        _ => None,
                    });

                (id, check_runs.collect::<Vec<_>>())
            })
            .collect::<Vec<_>>();

        Ok(result)
    }
}
