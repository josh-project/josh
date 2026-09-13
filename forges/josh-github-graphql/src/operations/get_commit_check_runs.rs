use crate::connection::GithubApiConnection;
use josh_github_codegen_graphql::{get_commit_check_runs, GetCommitCheckRuns};
use serde::{Deserialize, Serialize};

/// State of a single check run, folding its execution status and conclusion
/// into one persisted value.
///
/// Serialized as a plain string, not a derived unit-variant tree entry:
/// union-merge writes on changes refs cannot retract an old variant when a
/// check's state changes, while a same-named blob is overwritten cleanly.
/// See `PullRequestReviewState` in josh-github-webhooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// Queued or still running (no conclusion yet).
    Pending,
    Success,
    Failure,
    Neutral,
    Skipped,
    Cancelled,
    TimedOut,
    ActionRequired,
}

impl CheckState {
    /// Conservative admission rule: only an explicit success counts; neutral
    /// and skipped do not.
    pub fn passed(&self) -> bool {
        matches!(self, CheckState::Success)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CheckState::Pending => "pending",
            CheckState::Success => "success",
            CheckState::Failure => "failure",
            CheckState::Neutral => "neutral",
            CheckState::Skipped => "skipped",
            CheckState::Cancelled => "cancelled",
            CheckState::TimedOut => "timed_out",
            CheckState::ActionRequired => "action_required",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(CheckState::Pending),
            "success" => Some(CheckState::Success),
            "failure" => Some(CheckState::Failure),
            "neutral" => Some(CheckState::Neutral),
            "skipped" => Some(CheckState::Skipped),
            "cancelled" => Some(CheckState::Cancelled),
            "timed_out" => Some(CheckState::TimedOut),
            "action_required" => Some(CheckState::ActionRequired),
            _ => None,
        }
    }
}

impl Serialize for CheckState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CheckState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        CheckState::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown check state: {}", s)))
    }
}

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
