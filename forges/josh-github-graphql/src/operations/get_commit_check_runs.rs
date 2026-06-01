use crate::connection::GithubApiConnection;
use josh_github_codegen_graphql::{get_commit_check_runs, GetCommitCheckRuns};

impl GithubApiConnection {
    /// Fetch check run conclusions for a commit, returning `(name, passed)` for
    /// each check run found.
    pub async fn get_commit_check_runs(
        &self,
        owner: &str,
        name: &str,
        sha: &str,
    ) -> anyhow::Result<Vec<(String, bool)>> {
        let variables = get_commit_check_runs::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
            sha: sha.to_string(),
        };

        let response = self.make_request::<GetCommitCheckRuns>(variables).await?;

        let mut results = Vec::new();

        let Some(repo) = response.repository else {
            return Ok(results);
        };

        let Some(object) = repo.object else {
            return Ok(results);
        };

        let commit = match object {
            get_commit_check_runs::GetCommitCheckRunsRepositoryObject::Commit(c) => c,
            _ => return Ok(results),
        };

        let Some(check_suites) = commit.check_suites else {
            return Ok(results);
        };

        let Some(suite_nodes) = check_suites.nodes else {
            return Ok(results);
        };

        for suite_node in suite_nodes.into_iter().flatten() {
            let Some(check_runs_conn) = suite_node.check_runs else {
                continue;
            };
            let Some(run_nodes) = check_runs_conn.nodes else {
                continue;
            };
            for run_node in run_nodes.into_iter().flatten() {
                let passed = matches!(
                    run_node.conclusion,
                    Some(get_commit_check_runs::CheckConclusionState::Success)
                );
                results.push((run_node.name, passed));
            }
        }

        Ok(results)
    }
}
