use crate::connection::GithubApiConnection;

use josh_github_codegen_graphql::{get_default_branch, GetDefaultBranch};

impl GithubApiConnection {
    /// Returns (default_branch_name, default_branch_head_oid) if available.
    pub async fn get_default_branch(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        let variables = get_default_branch::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
        };

        let response = self.make_request::<GetDefaultBranch>(variables).await?;
        let repo = match response.repository {
            Some(r) => r,
            None => return Ok(None),
        };
        let default_ref = match repo.default_branch_ref {
            Some(r) => r,
            None => return Ok(None),
        };

        let target = match default_ref.target {
            Some(t) => t,
            None => return Ok(None),
        };

        Ok(Some((default_ref.name, target.oid)))
    }
}
