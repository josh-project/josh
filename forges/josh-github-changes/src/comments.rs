//! Posting local comments and votes to GitHub, converting fetched PR
//! comments into the forge-neutral shape `josh-changes` stores, and composing
//! `josh-changes` storage primitives with the GitHub ID tracking in `node_ids`.

use std::collections::HashMap;

use josh_core::cache::Transaction;
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
            timestamp: c.timestamp.clone(),
            reply_to: c.reply_to.clone(),
            meta: josh_changes::CommentMeta {
                message: c.body.clone(),
                file: c.path.clone(),
                // GitHub review comments address a single line; it maps to a
                // full-width `Location` on that line.
                location: c
                    .path
                    .as_ref()
                    .zip(c.line)
                    .map(|(_, line)| josh_changes::Location {
                        start_line: line as u32,
                        end_line: line as u32,
                        start_col: 1,
                        end_col: u32::MAX,
                    }),
                reply_to: None,
                update_of: None,
            },
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
            let meta = &comment.meta;
            let can_post = match &meta.reply_to {
                Some(parent_hash) => new_ids.contains_key(parent_hash.as_str()),
                None => true,
            };
            if !can_post {
                remaining.push(comment);
                continue;
            }

            let result = if let Some(ref file) = meta.file {
                if let Some(parent_hash) = &meta.reply_to {
                    match new_ids.get(parent_hash.as_str()) {
                        Some(parent_gh_id) => {
                            connection
                                .add_pull_request_review_thread_reply(parent_gh_id, &meta.message)
                                .await
                        }
                        None => {
                            remaining.push(comment);
                            continue;
                        }
                    }
                } else {
                    let line = meta
                        .location
                        .as_ref()
                        .map_or(1, |loc| loc.start_line as i64);
                    connection
                        .add_pull_request_review_thread(pr_node_id, &meta.message, file, line)
                        .await
                }
            } else {
                connection.add_comment(pr_node_id, &meta.message).await
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
                let meta = &comment.meta;
                let result = if meta.file.is_some() {
                    let line = meta
                        .location
                        .as_ref()
                        .map_or(1, |loc| loc.start_line as i64);
                    connection
                        .add_pull_request_review_thread(
                            pr_node_id,
                            &meta.message,
                            meta.file.as_deref().unwrap_or(""),
                            line,
                        )
                        .await
                } else {
                    connection.add_comment(pr_node_id, &meta.message).await
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
        let event = vote_state_to_github_review(&vote_data.state);
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

/// Map a josh vote state to a GitHub pull request review event.
fn vote_state_to_github_review(state: &str) -> &'static str {
    match state {
        "approve" => "APPROVE",
        "discuss" => "COMMENT",
        "revise" => "REQUEST_CHANGES",
        _ => "COMMENT",
    }
}

/// Filter `comments` (as loaded by [`josh_changes::read_comments`]) down to
/// those not yet posted to GitHub, plus the GitHub node IDs of already-posted
/// comments so [`post_comments`] can thread replies.
pub fn pending_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &josh_changes::ChangesRef,
    comments: Vec<josh_changes::Comment>,
) -> anyhow::Result<josh_changes::PendingComments> {
    // Pending comments live in `outbox/comments/...` on the Remote ref. Anything
    // already under `comments/...` was either fetched from the remote or has
    // already been posted; either way it should not be re-posted.
    let posted_ids = crate::node_ids::read_comment_node_ids(transaction, change_id, scope)?;
    let to_post = comments
        .into_iter()
        .filter(|c| c.pending && !posted_ids.contains_key(&c.id))
        .collect();

    Ok(josh_changes::PendingComments {
        to_post,
        posted_ids,
    })
}

/// Record GitHub metadata for comments just stored by
/// [`josh_changes::store_fetched_comments`]: track each written comment's
/// node ID so it is marked as already posted, and drop outbox entries whose
/// recorded node ID was observed in the fetch (the canonical copy now lives
/// under `comments/...` on the ref).
///
/// `written` is the `(local hash, GitHub node ID)` pair list returned by
/// `josh_changes::store_fetched_comments`.
pub fn record_fetched_comments(
    transaction: &Transaction,
    change_id: &str,
    written: &[(String, String)],
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<()> {
    crate::node_ids::store_comment_node_ids(transaction, change_id, written, scope)?;

    let fetched: std::collections::HashSet<&str> = written
        .iter()
        .map(|(_, github_id)| github_id.as_str())
        .collect();
    let gh_ids = crate::node_ids::read_comment_node_ids(transaction, change_id, scope)?;
    let to_remove: Vec<String> = gh_ids
        .into_iter()
        .filter(|(_, github_id)| fetched.contains(github_id.as_str()))
        .map(|(local_hash, _)| local_hash)
        .collect();
    josh_changes::delete_outbox_comments(transaction, change_id, scope, &to_remove)?;

    Ok(())
}

/// Filter `votes` (as loaded by [`josh_changes::list_outbox_votes`]) down to
/// those not yet posted to GitHub, i.e. whose `(state, sha)` is not already
/// recorded in `gh_vote_node_ids`.
pub fn pending_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &josh_changes::ChangesRef,
    votes: &[(String, josh_changes::VoteData)],
) -> anyhow::Result<Vec<(String, josh_changes::VoteData)>> {
    if votes.is_empty() {
        return Ok(Vec::new());
    }

    let tracked = crate::node_ids::read_vote_node_ids(transaction, change_id, scope)?;
    Ok(votes
        .iter()
        .filter(|(user, data)| match tracked.get(user) {
            Some(t) => t.state != data.state || t.sha != data.sha,
            None => true,
        })
        .cloned()
        .collect())
}

/// Remove outbox vote entries from `votes` (as loaded by
/// [`josh_changes::list_outbox_votes`]) whose `(state, sha)` is recorded in
/// `gh_vote_node_ids`, i.e. votes already posted to GitHub. Safe to call
/// unconditionally -- it's a no-op when nothing needs cleaning.
pub fn cleanup_posted_outbox_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &josh_changes::ChangesRef,
    votes: &[(String, josh_changes::VoteData)],
) -> anyhow::Result<()> {
    if votes.is_empty() {
        return Ok(());
    }

    let tracked = crate::node_ids::read_vote_node_ids(transaction, change_id, scope)?;
    if tracked.is_empty() {
        return Ok(());
    }

    let users: Vec<String> = votes
        .iter()
        .filter(|(user, data)| match tracked.get(user) {
            Some(t) => t.state == data.state && t.sha == data.sha,
            None => false,
        })
        .map(|(user, _)| user.clone())
        .collect();
    josh_changes::delete_outbox_votes(transaction, change_id, scope, &users)
}
