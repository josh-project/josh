use crate::connection::GithubApiConnection;
use josh_github_codegen_graphql::{get_commit_check_runs, GetCommitCheckRuns};

use josh_changes::CheckState;

impl GithubApiConnection {
    /// Fetch check run states for a commit, returning `(name, integration_id, state)`
    /// for each check run found.
    pub async fn get_commit_check_runs(
        &self,
        owner: &str,
        name: &str,
        sha: &str,
    ) -> anyhow::Result<Vec<(String, Option<i64>, CheckState)>> {
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
            let integration_id = suite_node
                .app
                .and_then(|app| app.database_id)
                .map(|id| id as i64);

            let Some(check_runs_conn) = suite_node.check_runs else {
                continue;
            };
            let Some(run_nodes) = check_runs_conn.nodes else {
                continue;
            };
            for run_node in run_nodes.into_iter().flatten() {
                use get_commit_check_runs::CheckConclusionState as Conclusion;
                let state = match run_node.conclusion {
                    Some(Conclusion::Success) => CheckState::Success,
                    Some(Conclusion::Failure) | Some(Conclusion::StartupFailure) => {
                        CheckState::Failure
                    }
                    Some(Conclusion::Neutral) => CheckState::Neutral,
                    Some(Conclusion::Skipped) => CheckState::Skipped,
                    Some(Conclusion::Cancelled) => CheckState::Cancelled,
                    Some(Conclusion::TimedOut) => CheckState::TimedOut,
                    Some(Conclusion::ActionRequired) => CheckState::ActionRequired,
                    _ => CheckState::Pending,
                };
                results.push((run_node.name, integration_id, state));
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_success_passes() {
        assert!(CheckState::Success.passed());
        for state in [
            CheckState::Pending,
            CheckState::Failure,
            CheckState::Neutral,
            CheckState::Skipped,
            CheckState::Cancelled,
            CheckState::TimedOut,
            CheckState::ActionRequired,
        ] {
            assert!(!state.passed());
        }
    }
}
