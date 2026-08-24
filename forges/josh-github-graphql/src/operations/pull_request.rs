use crate::connection::GithubApiConnection;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use josh_github_codegen_graphql::{
    add_comment, add_pull_request_review, add_pull_request_review_thread,
    add_pull_request_review_thread_reply, close_pull_request, convert_pull_request_to_draft,
    create_pull_request, get_pr_by_head, get_pr_comments, get_pr_reviews, get_prs_by_sha,
    list_open_p_rs, mark_pull_request_ready_for_review, update_pull_request, AddComment,
    AddPullRequestReview, AddPullRequestReviewThread, AddPullRequestReviewThreadReply,
    ClosePullRequest, ConvertPullRequestToDraft, CreatePullRequest, GetPrByHead, GetPrComments,
    GetPrReviews, GetPrsBySha, ListOpenPRs, MarkPullRequestReadyForReview, UpdatePullRequest,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrLabel {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone)]
pub struct PrComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub timestamp: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub reply_to: Option<String>,
    pub commit_oid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrData {
    pub title: String,
    pub body: Option<String>,
    pub number: i64,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    pub merged: bool,
    pub merged_at: Option<String>,
    pub merged_by: Option<String>,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub base_ref_name: String,
    pub head_ref_name: String,
    pub review_decision: Option<String>,
    pub check_status: Option<String>,
    // Sequences are unsupported by the git-tree format; labels are fetch-time
    // only and never persisted.
    #[serde(skip)]
    pub labels: Vec<PrLabel>,
    #[serde(skip)]
    pub comments: Vec<PrComment>,
}

#[derive(Debug)]
pub struct PrSummary {
    pub number: i64,
    pub title: String,
    pub body: String,
    pub base_ref_name: String,
    pub base_ref_oid: String,
    pub head_ref_name: String,
    pub head_oid: String,
    pub updated_at: String,
    pub comment_count: i64,
    pub review_count: i64,
    pub head_commit_message: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub pr_author_login: String,
}

impl GithubApiConnection {
    /// Find an open PR by head branch name. Returns (node_id, number, is_draft) if found.
    ///
    /// `head_owner`, when set, restricts the match to a PR whose head lives in
    /// that owner's repository — needed to disambiguate cross-fork PRs, where
    /// several forks may push the same head branch name. When `None`, the first
    /// open PR with the given head branch name is returned.
    pub async fn find_pull_request_by_head(
        &self,
        owner: &str,
        name: &str,
        head_ref_name: &str,
        head_owner: Option<&str>,
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
        let pr = nodes.into_iter().flatten().find(|n| match head_owner {
            Some(head_owner) => n
                .head_repository_owner
                .as_ref()
                .is_some_and(|o| o.login == head_owner),
            None => true,
        });
        Ok(pr.map(|n| (n.id, n.number, n.is_draft)))
    }

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

    /// List all open pull requests with pagination.
    pub async fn list_open_pull_requests(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Vec<PrSummary>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let variables = list_open_p_rs::Variables {
                owner: owner.to_string(),
                name: name.to_string(),
                first: 100,
                after: cursor.clone(),
            };
            let response = self.make_request::<ListOpenPRs>(variables).await?;
            let repo = match response.repository {
                Some(r) => r,
                None => return Err(anyhow!("repository not found")),
            };

            let nodes = repo.pull_requests.nodes.unwrap_or_default();
            for node in nodes.into_iter().flatten() {
                let head_commit = node
                    .commits
                    .nodes
                    .and_then(|ns| ns.into_iter().flatten().next())
                    .map(|n| n.commit);

                let (author_name, author_email) = match &head_commit {
                    Some(c) => (
                        c.author
                            .as_ref()
                            .and_then(|a| a.name.clone())
                            .unwrap_or_default(),
                        c.author
                            .as_ref()
                            .and_then(|a| a.email.clone())
                            .unwrap_or_default(),
                    ),
                    None => (String::new(), String::new()),
                };
                let (committer_name, committer_email) = match &head_commit {
                    Some(c) => (
                        c.committer
                            .as_ref()
                            .and_then(|c| c.name.clone())
                            .unwrap_or_default(),
                        c.committer
                            .as_ref()
                            .and_then(|c| c.email.clone())
                            .unwrap_or_default(),
                    ),
                    None => (String::new(), String::new()),
                };

                all.push(PrSummary {
                    number: node.number,
                    title: node.title,
                    body: node.body,
                    base_ref_name: node.base_ref_name,
                    base_ref_oid: node.base_ref_oid,
                    head_ref_name: node.head_ref_name,
                    head_oid: node.head_ref_oid,
                    updated_at: format!("{}", node.updated_at),
                    comment_count: node.comments.total_count,
                    review_count: node.reviews.map(|r| r.total_count).unwrap_or(0),
                    head_commit_message: head_commit.map(|c| c.message).unwrap_or_default(),
                    author_name,
                    author_email,
                    committer_name,
                    committer_email,
                    pr_author_login: node.author.map(|a| a.login).unwrap_or_default(),
                });
            }

            let page_info = repo.pull_requests.page_info;
            if page_info.has_next_page {
                cursor = page_info.end_cursor;
            } else {
                break;
            }
        }

        Ok(all)
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
                id: node.id.clone(),
                author: node.author.map(|a| a.login).unwrap_or_default(),
                body: node.body,
                timestamp: node.created_at.to_rfc3339(),
                path: None,
                line: None,
                reply_to: None,
                commit_oid: None,
            });
        }

        for review in pr
            .reviews
            .and_then(|r| r.nodes)
            .unwrap_or_default()
            .into_iter()
            .flatten()
        {
            let review_commit = review.commit.as_ref().map(|c| c.oid.clone());
            if !review.body.is_empty() {
                comments.push(PrComment {
                    id: review.id.clone(),
                    author: review
                        .author
                        .as_ref()
                        .map(|a| a.login.clone())
                        .unwrap_or_default(),
                    body: review.body,
                    timestamp: review.created_at.to_rfc3339(),
                    path: None,
                    line: None,
                    reply_to: None,
                    commit_oid: review_commit.clone(),
                });
            }
            for node in review
                .comments
                .nodes
                .unwrap_or_default()
                .into_iter()
                .flatten()
            {
                let node_commit = node.commit.as_ref().map(|c| c.oid.clone());
                comments.push(PrComment {
                    id: node.id.clone(),
                    author: node.author.map(|a| a.login).unwrap_or_default(),
                    body: node.body,
                    timestamp: node.created_at.to_rfc3339(),
                    path: Some(node.path),
                    line: node.line,
                    reply_to: node.reply_to.map(|r| r.id),
                    commit_oid: node_commit.or(review_commit.clone()),
                });
            }
        }

        let labels: Vec<PrLabel> = pr
            .labels
            .and_then(|l| l.nodes)
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .map(|n| PrLabel {
                name: n.name,
                color: n.color,
            })
            .collect();

        Ok(PrData {
            title: pr.title,
            body: Some(pr.body),
            number: pr.number,
            url: pr.url.to_string(),
            state: format!("{:?}", pr.state),
            is_draft: pr.is_draft,
            author: pr.author.map(|a| a.login).unwrap_or_default(),
            created_at: format!("{}", pr.created_at),
            updated_at: format!("{}", pr.updated_at),
            merged: pr.merged,
            merged_at: pr.merged_at.map(|t| format!("{}", t)),
            merged_by: pr.merged_by.map(|a| a.login),
            additions: pr.additions,
            deletions: pr.deletions,
            changed_files: pr.changed_files,
            base_ref_name: pr.base_ref_name,
            head_ref_name: pr.head_ref_name,
            review_decision: pr.review_decision.map(|r| format!("{:?}", r)),
            check_status: pr.status_check_rollup.map(|r| format!("{:?}", r.state)),
            labels,
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

    /// Post a general comment to a PR. Returns the GitHub comment node ID.
    pub async fn add_comment(&self, subject_id: &str, body: &str) -> anyhow::Result<String> {
        let variables = add_comment::Variables {
            subject_id: subject_id.to_string(),
            body: body.to_string(),
        };
        let response = self.make_request::<AddComment>(variables).await?;
        let node = response
            .add_comment
            .ok_or_else(|| anyhow!("addComment returned null"))?
            .comment_edge
            .ok_or_else(|| anyhow!("addComment commentEdge is null"))?
            .node
            .ok_or_else(|| anyhow!("addComment node is null"))?;
        Ok(node.id)
    }

    /// Post a file-level review thread comment. Returns the review thread node ID.
    pub async fn add_pull_request_review_thread(
        &self,
        pull_request_id: &str,
        body: &str,
        path: &str,
        line: i64,
    ) -> anyhow::Result<String> {
        let variables = add_pull_request_review_thread::Variables {
            pull_request_id: pull_request_id.to_string(),
            body: body.to_string(),
            path: path.to_string(),
            line,
            side: add_pull_request_review_thread::DiffSide::Right,
            subject_type: add_pull_request_review_thread::PullRequestReviewThreadSubjectType::Line,
        };
        let response = self
            .make_request::<AddPullRequestReviewThread>(variables)
            .await?;
        let thread = response
            .add_pull_request_review_thread
            .ok_or_else(|| anyhow!("addPullRequestReviewThread returned null"))?
            .thread
            .ok_or_else(|| anyhow!("addPullRequestReviewThread thread is null"))?;
        Ok(thread.id)
    }

    /// Reply to an existing review thread. Returns the reply comment node ID.
    pub async fn add_pull_request_review_thread_reply(
        &self,
        thread_id: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let variables = add_pull_request_review_thread_reply::Variables {
            pull_request_review_thread_id: thread_id.to_string(),
            body: body.to_string(),
        };
        let response = self
            .make_request::<AddPullRequestReviewThreadReply>(variables)
            .await?;
        let comment = response
            .add_pull_request_review_thread_reply
            .ok_or_else(|| anyhow!("addPullRequestReviewThreadReply returned null"))?
            .comment
            .ok_or_else(|| anyhow!("addPullRequestReviewThreadReply comment is null"))?;
        Ok(comment.id)
    }

    /// Submit a pull request review (approve, comment, or request changes).
    /// Returns the review node ID.
    pub async fn add_pull_request_review(
        &self,
        pull_request_id: &str,
        event: &str,
        body: Option<&str>,
        commit_oid: Option<&str>,
    ) -> anyhow::Result<String> {
        let event = match event {
            "APPROVE" => add_pull_request_review::PullRequestReviewEvent::Approve,
            "COMMENT" => add_pull_request_review::PullRequestReviewEvent::Comment,
            "REQUEST_CHANGES" => add_pull_request_review::PullRequestReviewEvent::RequestChanges,
            other => return Err(anyhow!("unknown review event: {}", other)),
        };
        let variables = add_pull_request_review::Variables {
            pull_request_id: pull_request_id.to_string(),
            event,
            body: body.map(String::from),
            commit_oid: commit_oid.map(String::from),
        };
        let response = self.make_request::<AddPullRequestReview>(variables).await?;
        let review = response
            .add_pull_request_review
            .ok_or_else(|| anyhow!("addPullRequestReview returned null"))?
            .pull_request_review
            .ok_or_else(|| anyhow!("addPullRequestReview pullRequestReview is null"))?;
        Ok(review.id)
    }

    /// Returns the latest review state for each reviewer on a PR.
    pub async fn get_pr_reviews(
        &self,
        owner: &str,
        name: &str,
        pr_number: i64,
    ) -> anyhow::Result<
        Vec<(
            String,
            josh_github_webhooks::webhook_types::PullRequestReviewState,
        )>,
    > {
        let mut reviews: std::collections::HashMap<
            String,
            josh_github_webhooks::webhook_types::PullRequestReviewState,
        > = std::collections::HashMap::new();
        let mut cursor: Option<String> = None;

        loop {
            let variables = get_pr_reviews::Variables {
                owner: owner.to_string(),
                name: name.to_string(),
                number: pr_number,
                first: 100,
                after: cursor,
            };

            let response = self.make_request::<GetPrReviews>(variables).await?;

            let repo = match response.repository {
                Some(r) => r,
                None => break,
            };
            let pr = match repo.pull_request {
                Some(p) => p,
                None => break,
            };
            let reviews_conn = match pr.reviews {
                Some(r) => r,
                None => break,
            };

            if let Some(nodes) = reviews_conn.nodes {
                for node in nodes.into_iter().flatten() {
                    let login = node
                        .author
                        .map(|a| a.login)
                        .unwrap_or_else(|| "unknown".to_string());
                    let state = match node.state {
                        get_pr_reviews::PullRequestReviewState::Approved => {
                            josh_github_webhooks::webhook_types::PullRequestReviewState::Approved
                        }
                        get_pr_reviews::PullRequestReviewState::ChangesRequested => {
                            josh_github_webhooks::webhook_types::PullRequestReviewState::ChangesRequested
                        }
                        get_pr_reviews::PullRequestReviewState::Commented => {
                            josh_github_webhooks::webhook_types::PullRequestReviewState::Commented
                        }
                        get_pr_reviews::PullRequestReviewState::Dismissed => {
                            josh_github_webhooks::webhook_types::PullRequestReviewState::Dismissed
                        }
                        _ => continue,
                    };
                    reviews.insert(login, state);
                }
            }

            if reviews_conn.page_info.has_next_page {
                cursor = reviews_conn.page_info.end_cursor;
            } else {
                break;
            }
        }

        Ok(reviews.into_iter().collect())
    }
}
