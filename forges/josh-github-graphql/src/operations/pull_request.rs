use crate::connection::GithubApiConnection;
use anyhow::anyhow;

use josh_github_codegen_graphql::{
    close_pull_request, convert_pull_request_to_draft, create_pull_request, get_open_prs,
    get_pr_by_head, get_prs_by_sha, mark_pull_request_ready_for_review, update_pull_request,
    ClosePullRequest, ConvertPullRequestToDraft, CreatePullRequest, GetOpenPrs, GetPrByHead,
    GetPrsBySha, MarkPullRequestReadyForReview, UpdatePullRequest,
};

/// An open pull request discovered during fetch.
#[derive(Debug, Clone)]
pub struct OpenPr {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub head_sha: String,
    pub head_branch: String,
    pub base_sha: String,
    pub base_branch: String,
}

impl GithubApiConnection {
    /// Find open PRs whose head commit is the given SHA. Returns `(node_id, number)` for each.
    pub async fn find_open_prs_by_head_sha(
        &self,
        owner: &str,
        name: &str,
        sha: &str,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        let variables = get_prs_by_sha::Variables {
            owner: owner.to_string(),
            name: name.to_string(),
            sha: sha.to_string(),
        };
        let response = self.make_request::<GetPrsBySha>(variables).await?;
        let prs = response
            .repository
            .and_then(|r| r.object)
            .and_then(|o| match o {
                get_prs_by_sha::GetPrsByShaRepositoryObject::Commit(c) => {
                    c.associated_pull_requests
                }
                _ => None,
            })
            .and_then(|p| p.nodes)
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .map(|n| (n.id, n.number))
            .collect();
        Ok(prs)
    }

    /// List all open pull requests for a repository.
    pub async fn get_open_pull_requests(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Vec<OpenPr>> {
        let mut prs = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let variables = get_open_prs::Variables {
                owner: owner.to_string(),
                name: name.to_string(),
                first: 100,
                after: cursor,
            };

            let response = self.make_request::<GetOpenPrs>(variables).await?;

            let pull_requests = match response.repository {
                Some(repo) => repo.pull_requests,
                None => break,
            };

            if let Some(nodes) = pull_requests.nodes {
                for node in nodes.into_iter().flatten() {
                    prs.push(OpenPr {
                        node_id: node.id,
                        number: node.number,
                        title: node.title,
                        head_sha: node.head_ref_oid,
                        head_branch: node.head_ref_name,
                        base_sha: node.base_ref_oid,
                        base_branch: node.base_ref_name,
                    });
                }
            }

            if pull_requests.page_info.has_next_page {
                cursor = pull_requests.page_info.end_cursor;
            } else {
                break;
            }
        }

        Ok(prs)
    }

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
