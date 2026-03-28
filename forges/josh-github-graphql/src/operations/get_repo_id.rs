use anyhow::Context;

use crate::connection::GithubApiConnection;
use josh_github_codegen_graphql::{get_repo_id, GetRepoId};

impl GithubApiConnection {
    pub async fn get_repo_id(&self, owner: &str, name: &str) -> anyhow::Result<String> {
        let variables = get_repo_id::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
        };

        let response = self.make_request::<GetRepoId>(variables).await?;
        Ok(response.repository.context("Empty repository field")?.id)
    }
}
