use crate::connection::GithubApiConnection;
use anyhow::anyhow;

use josh_github_codegen_graphql::{
    close_pull_request, convert_pull_request_to_draft, create_pull_request, get_pr_by_head,
    get_pr_comments, mark_pull_request_ready_for_review, update_pull_request, ClosePullRequest,
    ConvertPullRequestToDraft, CreatePullRequest, GetPrByHead, GetPrComments,
    MarkPullRequestReadyForReview, UpdatePullRequest,
};

#[derive(Debug)]
pub struct PrComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub timestamp: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub reply_to: Option<String>,
}

#[derive(Debug)]
pub struct PrData {
    pub title: String,
    pub body: Option<String>,
    pub author: String,
    pub timestamp: String,
    pub comments: Vec<PrComment>,
}

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

    pub async fn get_pr_comments(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> anyhow::Result<PrData> {
        let variables = get_pr_comments::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
            number,
        };
        let response = self.make_request::<GetPrComments>(variables).await?;
        let repo = response
            .repository
            .ok_or_else(|| anyhow!("repository not found"))?;
        let pr = repo.pull_request.ok_or_else(|| anyhow!("PR not found"))?;

        let mut comments = Vec::new();
        for node in pr.comments.nodes.unwrap_or_default().into_iter().flatten() {
            comments.push(PrComment {
                id: node.id,
                author: node.author.map(|a| a.login).unwrap_or_default(),
                body: node.body,
                timestamp: format!("{}", node.created_at),
                path: None,
                line: None,
                reply_to: None,
            });
        }

        for review in pr
            .reviews
            .and_then(|r| r.nodes)
            .unwrap_or_default()
            .into_iter()
            .flatten()
        {
            if !review.body.is_empty() {
                comments.push(PrComment {
                    id: review.id,
                    author: review
                        .author
                        .as_ref()
                        .map(|a| a.login.clone())
                        .unwrap_or_default(),
                    body: review.body,
                    timestamp: format!("{}", review.created_at),
                    path: None,
                    line: None,
                    reply_to: None,
                });
            }
            for node in review
                .comments
                .nodes
                .unwrap_or_default()
                .into_iter()
                .flatten()
            {
                comments.push(PrComment {
                    id: node.id,
                    author: node.author.map(|a| a.login).unwrap_or_default(),
                    body: node.body,
                    timestamp: format!("{}", node.created_at),
                    path: Some(node.path),
                    line: node.line,
                    reply_to: node.reply_to.map(|r| r.id),
                });
            }
        }

        Ok(PrData {
            title: pr.title,
            body: Some(pr.body),
            author: pr.author.map(|a| a.login).unwrap_or_default(),
            timestamp: format!("{}", pr.created_at),
            comments,
        })
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
