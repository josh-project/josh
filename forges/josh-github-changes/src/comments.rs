//! Posting local comments and votes to GitHub, and converting fetched PR
//! comments into the forge-neutral shape `josh-changes` stores.

use std::collections::HashMap;

use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::pull_request::PrData;

/// Convert fetched GitHub PR comments into the forge-neutral shape
/// `josh_changes::store_fetched_comments` consumes.
pub fn fetched_comments(pr_data: &PrData) -> Vec<josh_changes::FetchedComment> {
    pr_data
        .comments
        .iter()
        .map(|c| josh_changes::FetchedComment {
            forge_id: c.id.clone(),
            author: c.author.clone(),
            body: c.body.clone(),
            timestamp: c.timestamp.clone(),
            path: c.path.clone(),
            line: c.line,
            reply_to: c.reply_to.clone(),
            commit_oid: c.commit_oid.clone(),
        })
        .collect()
}

/// A comment posted to GitHub: the local content hash paired with the GitHub
/// node ID it was posted as.
pub struct PostedComment {
    pub local_id: String,
    pub github_id: String,
}

/// Result of a [`post_comments`] run: everything posted before an eventual
/// failure, plus the first error encountered (posting stops at that point).
pub struct PostCommentsOutcome {
    pub posted: Vec<PostedComment>,
    pub error: Option<anyhow::Error>,
}

/// Post pending comments to a GitHub PR. Recording the returned IDs into
/// local refs is the caller's job.
pub async fn post_comments(
    connection: &GithubApiConnection,
    pr_node_id: &str,
    pending: &josh_changes::PendingComments,
) -> PostCommentsOutcome {
    let mut outcome = PostCommentsOutcome {
        posted: Vec::new(),
        error: None,
    };

    // Topological sort: post parents before children that reply to them.
    let mut new_ids: HashMap<String, String> = pending.posted_ids.clone();
    let mut unposted: Vec<&josh_changes::Comment> = pending.to_post.iter().collect();

    while !unposted.is_empty() {
        let mut progressed = false;
        let mut remaining = Vec::new();

        for comment in unposted.drain(..) {
            let can_post = match &comment.reply_to {
                Some(parent_hash) => new_ids.contains_key(parent_hash.as_str()),
                None => true,
            };
            if !can_post {
                remaining.push(comment);
                continue;
            }

            let result = if let Some(ref file) = comment.file {
                if let Some(parent_hash) = &comment.reply_to {
                    match new_ids.get(parent_hash.as_str()) {
                        Some(parent_gh_id) => {
                            connection
                                .add_pull_request_review_thread_reply(
                                    parent_gh_id,
                                    &comment.message,
                                )
                                .await
                        }
                        None => {
                            remaining.push(comment);
                            continue;
                        }
                    }
                } else {
                    let line = comment
                        .location
                        .as_ref()
                        .map_or(1, |loc| loc.start_line as i64);
                    connection
                        .add_pull_request_review_thread(pr_node_id, &comment.message, file, line)
                        .await
                }
            } else {
                connection.add_comment(pr_node_id, &comment.message).await
            };

            match result {
                Ok(github_id) => {
                    new_ids.insert(comment.id.clone(), github_id.clone());
                    outcome.posted.push(PostedComment {
                        local_id: comment.id.clone(),
                        github_id,
                    });
                    progressed = true;
                }
                Err(e) => {
                    outcome.error = Some(e);
                    return outcome;
                }
            }
        }

        if !progressed {
            // Orphan reply_to references — post remaining as standalone.
            for comment in remaining.drain(..) {
                let result = if comment.file.is_some() {
                    let line = comment
                        .location
                        .as_ref()
                        .map_or(1, |loc| loc.start_line as i64);
                    connection
                        .add_pull_request_review_thread(
                            pr_node_id,
                            &comment.message,
                            comment.file.as_deref().unwrap_or(""),
                            line,
                        )
                        .await
                } else {
                    connection.add_comment(pr_node_id, &comment.message).await
                };
                match result {
                    Ok(github_id) => {
                        new_ids.insert(comment.id.clone(), github_id.clone());
                        outcome.posted.push(PostedComment {
                            local_id: comment.id.clone(),
                            github_id,
                        });
                    }
                    Err(e) => {
                        outcome.error = Some(e);
                        return outcome;
                    }
                }
            }
            break;
        }

        unposted = remaining;
    }

    outcome
}

/// Result of a [`post_votes`] run: every vote posted before an eventual
/// failure, plus the first error encountered (posting stops at that point).
pub struct PostVotesOutcome {
    pub posted: Vec<(String, josh_changes::VoteData)>,
    pub error: Option<anyhow::Error>,
}

/// Post pending votes to a GitHub PR as pull request reviews. Recording the
/// returned votes into local refs is the caller's job.
pub async fn post_votes(
    connection: &GithubApiConnection,
    pr_node_id: &str,
    commit_oid: &str,
    votes: &[(String, josh_changes::VoteData)],
) -> PostVotesOutcome {
    let mut outcome = PostVotesOutcome {
        posted: Vec::new(),
        error: None,
    };

    for (user, vote_data) in votes {
        let event = josh_changes::vote_state_to_github_review(&vote_data.state);
        let body = format!("josh vote: {}", vote_data.state);

        match connection
            .add_pull_request_review(pr_node_id, event, Some(&body), Some(commit_oid))
            .await
        {
            Ok(_review_id) => outcome.posted.push((user.clone(), vote_data.clone())),
            Err(e) => {
                outcome.error = Some(e);
                return outcome;
            }
        }
    }

    outcome
}
