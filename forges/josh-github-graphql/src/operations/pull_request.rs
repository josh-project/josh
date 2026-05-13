use crate::connection::GithubApiConnection;
use anyhow::anyhow;

use josh_github_codegen_graphql::{
    close_pull_request, convert_pull_request_to_draft, create_pull_request, get_pr_by_head,
    mark_pull_request_ready_for_review, update_pull_request, ClosePullRequest,
    ConvertPullRequestToDraft, CreatePullRequest, GetPrByHead, MarkPullRequestReadyForReview,
    UpdatePullRequest,
};

impl GithubApiConnection {
    /// Find an open PR by head branch name. Returns (node_id, number, is_draft) if found.
    pub async fn find_pull_request_by_head(
        &self,
        owner: &str,
        name: &str,
        head_ref_name: &str,
    ) -> anyhow::Result<Option<(String, i64, bool)>> {
        let variables = get_pr_by_head::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
            head_ref_name: head_ref_name.to_string(),
        };
        let response = self.make_request::<GetPrByHead>(variables).await?;
        let repo = match response.repository {
            Some(r) => r,
            None => return Ok(None),
        };
        let nodes = repo.pull_requests.nodes.unwrap_or_default();
        let pr = nodes.into_iter().flatten().next();
        Ok(pr.map(|n| (n.id, n.number, n.is_draft)))
    }

    /// Update an existing PR's title, body, and/or base branch.
    pub async fn update_pull_request(
        &self,
        pull_request_id: &str,
        title: Option<&str>,
        body: Option<&str>,
        base_ref_name: Option<&str>,
    ) -> anyhow::Result<(String, i64)> {
        let variables = update_pull_request::Variables {
            pull_request_id: pull_request_id.to_string(),
            title: title.map(String::from),
            body: body.map(String::from),
            base_ref_name: base_ref_name.map(String::from),
        };
        let response = self.make_request::<UpdatePullRequest>(variables).await?;
        let response = match response.update_pull_request {
            Some(r) => r,
            None => return Err(anyhow!("Failed to parse response: update_pull_request")),
        };
        let pr = match response.pull_request {
            Some(p) => p,
            None => return Err(anyhow!("Failed to parse response: pull_request")),
        };
        Ok((pr.id, pr.number))
    }

    pub async fn convert_pull_request_to_draft(
        &self,
        pull_request_id: &str,
    ) -> anyhow::Result<(String, i64, bool)> {
        let variables = convert_pull_request_to_draft::Variables {
            pull_request_id: pull_request_id.to_string(),
        };
        let response = self
            .make_request::<ConvertPullRequestToDraft>(variables)
            .await?;
        let response = match response.convert_pull_request_to_draft {
            Some(r) => r,
            None => {
                return Err(anyhow!(
                    "Failed to parse response: convert_pull_request_to_draft"
                ))
            }
        };
        let pr = match response.pull_request {
            Some(p) => p,
            None => return Err(anyhow!("Failed to parse response: pull_request")),
        };
        Ok((pr.id, pr.number, pr.is_draft))
    }

    pub async fn mark_pull_request_ready_for_review(
        &self,
        pull_request_id: &str,
    ) -> anyhow::Result<(String, i64, bool)> {
        let variables = mark_pull_request_ready_for_review::Variables {
            pull_request_id: pull_request_id.to_string(),
        };
        let response = self
            .make_request::<MarkPullRequestReadyForReview>(variables)
            .await?;
        let response = match response.mark_pull_request_ready_for_review {
            Some(r) => r,
            None => {
                return Err(anyhow!(
                    "Failed to parse response: mark_pull_request_ready_for_review"
                ))
            }
        };
        let pr = match response.pull_request {
            Some(p) => p,
            None => return Err(anyhow!("Failed to parse response: pull_request")),
        };
        Ok((pr.id, pr.number, pr.is_draft))
    }

    pub async fn create_pull_request(
        &self,
        repository_id: &str,
        base_branch: &str,
        head_branch: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> anyhow::Result<(String, i64)> {
        let variables = create_pull_request::Variables {
            repository_id: repository_id.to_string(),
            base_ref_name: base_branch.into(),
            head_ref_name: head_branch.into(),
            title: title.to_string(),
            body: body.to_string(),
            draft,
        };

        let response = self.make_request::<CreatePullRequest>(variables).await?;
        let response = match response.create_pull_request {
            Some(response) => response,
            None => return Err(anyhow!("Failed to parse response: create_pull_request")),
        };

        let response = match response.pull_request {
            Some(response) => response,
            None => return Err(anyhow!("Failed to parse response: pull_request")),
        };

        Ok((response.id, response.number))
    }

    pub async fn close_pull_request(
        &self,
        // Note: this is not pull request number! This is a global "node id"
        pull_request_node_id: &str,
    ) -> anyhow::Result<()> {
        let variables = close_pull_request::Variables {
            pull_request_node_id: pull_request_node_id.to_string(),
        };

        let response = self.make_request::<ClosePullRequest>(variables).await?;
        if response.close_pull_request.is_none() {
            return Err(anyhow!("Failed to parse response: close_pull_request"));
        };

        Ok(())
    }
}
