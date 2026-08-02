//! Syncing PR comments from GitHub and posting local comments and votes.

use std::collections::HashMap;

use josh_github_graphql::connection::GithubApiConnection;

/// Write PR comments into the given changes ref. Shared by both sync paths.
fn write_pr_comments(
    repo: &git2::Repository,
    change: &josh_changes::Change,
    pr_data: &josh_github_graphql::operations::pull_request::PrData,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<usize> {
    let mut id_map: HashMap<String, String> = HashMap::new();
    for comment in &pr_data.comments {
        let location =
            comment
                .path
                .as_ref()
                .zip(comment.line)
                .map(|(_, line)| josh_changes::Location {
                    start_line: line as u32,
                    end_line: line as u32,
                    start_col: 1,
                    end_col: u32::MAX,
                });
        let reply_to = comment
            .reply_to
            .as_ref()
            .and_then(|gh_id| id_map.get(gh_id))
            .cloned();
        let meta = josh_changes::CommentMeta {
            message: comment.body.clone(),
            file: comment.path.clone(),
            location,
            reply_to,
            update_of: None,
        };

        let blob_commit = comment.commit_oid.clone();

        let hash = josh_changes::write_comment_with_commit(
            repo,
            change,
            &meta,
            Some(&comment.author),
            Some(&comment.timestamp),
            blob_commit.as_deref(),
            scope,
        )?;
        // Record the GitHub node ID so this comment is tracked as "already posted".
        if let Some(change_id) = change.id() {
            josh_changes::store_github_id(repo, change_id, &hash, &comment.id, scope)?;
        }
        id_map.insert(comment.id.clone(), hash);
    }

    // Cleanup: any outbox entry whose `gh_ids[hash]` points at a GitHub node
    // we just observed in the fetch can now be dropped — the canonical copy
    // lives under `comments/...` on this ref.
    if let Some(change_id) = change.id() {
        let fetched: std::collections::HashSet<&str> =
            pr_data.comments.iter().map(|c| c.id.as_str()).collect();
        let gh_ids = josh_changes::read_github_ids(repo, change_id, scope)?;
        let to_remove: Vec<String> = gh_ids
            .into_iter()
            .filter(|(_, gh_id)| fetched.contains(gh_id.as_str()))
            .map(|(local_hash, _)| local_hash)
            .collect();
        josh_changes::delete_outbox_comments(repo, change_id, scope, &to_remove)?;
    }

    Ok(pr_data.comments.len())
}

/// Sync GitHub PR comments for a single change into the given remote changes ref.
/// Returns the number of comments synced.
pub async fn sync_change_comments(
    connection: &GithubApiConnection,
    owner: &str,
    repo_name: &str,
    repo: &git2::Repository,
    change: &josh_changes::Change,
    head_ref: &str,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<usize> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(0),
    };

    let pr = match connection
        .find_pull_request_by_head(owner, repo_name, head_ref, None)
        .await?
    {
        Some((_node_id, number, _draft)) => {
            println!("Found PR #{} for change {}", number, change_id);
            number
        }
        None => {
            eprintln!(
                "No open PR found for change {} (branch {})",
                change_id, head_ref
            );
            return Ok(0);
        }
    };

    let pr_data = connection.get_pr_comments(owner, repo_name, pr).await?;
    let json = serde_json::to_string(&pr_data)?;
    josh_changes::store_pr_data(repo, change_id, &json, scope)?;

    write_pr_comments(repo, change, &pr_data, scope)
}

/// Sync GitHub PR comments for a change identified directly by PR number.
pub async fn sync_change_comments_by_pr_number(
    connection: &GithubApiConnection,
    owner: &str,
    repo_name: &str,
    repo: &git2::Repository,
    change: &josh_changes::Change,
    pr_number: i64,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<usize> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(0),
    };

    let pr_data = connection
        .get_pr_comments(owner, repo_name, pr_number)
        .await?;
    let json = serde_json::to_string(&pr_data)?;
    josh_changes::store_pr_data(repo, change_id, &json, scope)?;

    write_pr_comments(repo, change, &pr_data, scope)
}

/// Post local comments (those without a `github_id`) to a GitHub PR.
/// Reads drafts from `local_scope`, dedupes against the union of `gh_ids` maps
/// across all refs, and writes new mappings into `remote_scope`.
/// Returns the number of comments successfully posted.
pub async fn post_local_comments(
    connection: &GithubApiConnection,
    repo: &git2::Repository,
    change_id: &str,
    pr_node_id: &str,
    remote_scope: &josh_changes::ChangesRef,
) -> anyhow::Result<usize> {
    // Pending comments live in `outbox/comments/...` on the Remote ref. Anything
    // already under `comments/...` was either fetched from the remote or has
    // already been posted; either way it should not be re-posted.
    let comments: Vec<josh_changes::Comment> =
        josh_changes::read_comments(repo, change_id, remote_scope)?
            .into_iter()
            .filter(|c| c.pending)
            .collect();
    if comments.is_empty() {
        return Ok(0);
    }

    let github_ids = josh_changes::read_github_ids_union(repo, change_id)?;

    // Collect unposted comments (no github_id mapping yet).
    let mut unposted: Vec<&josh_changes::Comment> = comments
        .iter()
        .filter(|c| !github_ids.contains_key(&c.id))
        .collect();
    if unposted.is_empty() {
        return Ok(0);
    }

    // Topological sort: post parents before children that reply to them.
    let mut posted_count = 0usize;
    let mut new_ids: std::collections::HashMap<String, String> = github_ids;

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

            let github_id = if let Some(ref file) = comment.file {
                if let Some(parent_hash) = &comment.reply_to {
                    let parent_gh_id = match new_ids.get(parent_hash.as_str()) {
                        Some(id) => id,
                        None => {
                            remaining.push(comment);
                            continue;
                        }
                    };
                    connection
                        .add_pull_request_review_thread_reply(parent_gh_id, &comment.message)
                        .await?
                } else {
                    let line = comment
                        .location
                        .as_ref()
                        .map_or(1, |loc| loc.start_line as i64);
                    connection
                        .add_pull_request_review_thread(pr_node_id, &comment.message, file, line)
                        .await?
                }
            } else {
                connection.add_comment(pr_node_id, &comment.message).await?
            };

            josh_changes::store_github_id(repo, change_id, &comment.id, &github_id, remote_scope)?;
            new_ids.insert(comment.id.clone(), github_id);
            posted_count += 1;
            progressed = true;
        }

        if !progressed {
            // Orphan reply_to references — post remaining as standalone.
            for comment in remaining.drain(..) {
                let github_id = if comment.file.is_some() {
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
                        .await?
                } else {
                    connection.add_comment(pr_node_id, &comment.message).await?
                };
                josh_changes::store_github_id(
                    repo,
                    change_id,
                    &comment.id,
                    &github_id,
                    remote_scope,
                )?;
                new_ids.insert(comment.id.clone(), github_id);
                posted_count += 1;
            }
            break;
        }

        unposted = remaining;
    }

    Ok(posted_count)
}

/// Post local votes (those not yet pushed to GitHub) as pull request reviews.
/// Reads from `local_scope`, dedupes against the remote's `gh_vote_ids`, and
/// writes the tracking entry into `remote_scope`.
pub async fn post_local_votes(
    connection: &GithubApiConnection,
    repo: &git2::Repository,
    change_id: &str,
    pr_node_id: &str,
    commit_oid: &str,
    remote_scope: &josh_changes::ChangesRef,
) -> anyhow::Result<usize> {
    let votes = josh_changes::list_outbox_votes(repo, change_id, remote_scope)?;
    if votes.is_empty() {
        return Ok(0);
    }

    let tracked = josh_changes::read_github_vote_ids(repo, change_id, remote_scope)?;

    let mut posted = 0usize;
    for (user, vote_data) in &votes {
        if let Some(tracked_data) = tracked.get(user) {
            if tracked_data.state == vote_data.state && tracked_data.sha == vote_data.sha {
                continue;
            }
        }

        let event = josh_changes::vote_state_to_github_review(&vote_data.state);
        let body = format!("josh vote: {}", vote_data.state);

        let _review_id = connection
            .add_pull_request_review(pr_node_id, event, Some(&body), Some(commit_oid))
            .await?;

        josh_changes::store_github_vote_id(repo, change_id, user, vote_data, remote_scope)?;
        posted += 1;
    }

    // Drop outbox entries whose post is now reflected in gh_vote_ids. Safe to
    // call unconditionally -- it's a no-op when nothing needs cleaning.
    josh_changes::cleanup_posted_outbox_votes(repo, change_id, remote_scope)?;

    Ok(posted)
}
