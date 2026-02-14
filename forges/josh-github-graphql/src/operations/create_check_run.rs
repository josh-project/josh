use anyhow::Context;
use url::Url;

use josh_github_codegen_graphql::{create_check_run, CreateCheckRun};

use crate::connection::GithubApiConnection;

impl GithubApiConnection {
    pub async fn create_check_run(
        &self,
        head_sha: &git2::Oid,
        name: &str,
        repository_id: &str,
        status: create_check_run::RequestableCheckStatusState,
        conclusion: create_check_run::CheckConclusionState,
        details_url: &Url,
    ) -> anyhow::Result<String> {
        let variables = create_check_run::Variables {
            input: create_check_run::CreateCheckRunInput {
                actions: None,
                client_mutation_id: None,
                completed_at: None,
                conclusion: Some(conclusion),
                details_url: Some(details_url.clone()),
                external_id: None,
                head_sha: head_sha.to_string(),
                name: name.to_string(),
                output: None,
                repository_id: repository_id.to_string(),
                started_at: None,
                status: Some(status),
            },
        };

        let response = self.make_request::<CreateCheckRun>(variables).await?;

        Ok(response
            .create_check_run
            .context("Could not create checkrun")?
            .check_run
            .context("Could not create checkrun")?
            .id)
    }
}
