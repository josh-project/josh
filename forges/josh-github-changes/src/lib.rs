pub mod admission;
pub mod repo;

use std::collections::HashMap;

use josh_github_graphql::connection::GithubApiConnection;

#[derive(Debug)]
pub struct PrInfo {
    pub head_branch: String,
    pub base_branch: String,
    pub base_oid: git2::Oid,
    pub title: String,
    pub body: String,
}

/// Collect PR info from a set of refs to push.
/// Uses the @base ref for each change as the base branch. Title and body come from the head commit message.
pub fn collect_pr_infos(repo: &git2::Repository, to_push: &[josh_changes::PushRef]) -> Vec<PrInfo> {
    #[derive(Default)]
    struct ByIdEntry {
        head_branch: Option<String>,
        base_branch: Option<String>,
        head_oid: Option<git2::Oid>,
        base_oid: Option<git2::Oid>,
    }

    fn branch_name(refname: &str) -> &str {
        refname.strip_prefix("refs/heads/").unwrap_or(refname)
    }

    let mut by_change_id: HashMap<String, ByIdEntry> = HashMap::new();
    for push_ref in to_push {
        let branch = branch_name(&push_ref.ref_name).to_string();
        if push_ref.ref_name.contains("@changes") {
            let entry = by_change_id.entry(push_ref.change_id.clone()).or_default();
            entry.head_branch = Some(branch);
            entry.head_oid = Some(push_ref.oid);
        } else if push_ref.ref_name.contains("@base") {
            let entry = by_change_id.entry(push_ref.change_id.clone()).or_default();
            entry.base_branch = Some(branch);
            entry.base_oid = Some(push_ref.oid);
        }
    }

    by_change_id
        .into_iter()
        .filter_map(|(_, entry)| {
            let (head, base, head_oid, base_oid) = (
                entry.head_branch?,
                entry.base_branch?,
                entry.head_oid?,
                entry.base_oid?,
            );
            let commit = repo.find_commit(head_oid).ok()?;
            let raw_message = commit.message().unwrap_or("");
            let message = raw_message.trim_end();
            let title = message.lines().next().unwrap_or("").trim().to_string();
            let title = if title.is_empty() {
                format!("{} → {}", head, base)
            } else {
                title
            };
            let body = message.to_string();
            Some(PrInfo {
                head_branch: head,
                base_branch: base,
                base_oid,
                title,
                body,
            })
        })
        .collect()
}

pub async fn create_or_update_prs(
    connection: &GithubApiConnection,
    url: &str,
    pr_infos: &[PrInfo],
    dry_run: bool,
) -> anyhow::Result<()> {
    let (owner, repo_name) = crate::repo::parse_owner_repo(url)?;

    let repository_id = connection.get_repo_id(&owner, &repo_name).await?;
    let default_branch = connection.get_default_branch(&owner, &repo_name).await?;

    for info in pr_infos {
        let effective_base_branch = match &default_branch {
            Some((default_name, default_oid)) if info.base_oid.to_string() == *default_oid => {
                default_name.as_str()
            }
            _ => info.base_branch.as_str(),
        };
        let desired_draft = match &default_branch {
            Some((default_name, _)) => effective_base_branch != default_name.as_str(),
            None => effective_base_branch == info.base_branch.as_str(),
        };

        if dry_run {
            match connection
                .find_pull_request_by_head(&owner, &repo_name, &info.head_branch)
                .await
            {
                Ok(Some((_, number, is_draft))) => eprintln!(
                    "Would update PR #{}: {} → {} (draft: {} → {})",
                    number, info.head_branch, effective_base_branch, is_draft, desired_draft
                ),
                Ok(None) => eprintln!(
                    "Would create PR: {} → {} (draft: {})",
                    info.head_branch, effective_base_branch, desired_draft
                ),
                Err(e) => eprintln!("Failed to look up PR for {}: {}", info.head_branch, e),
            }
            continue;
        }

        match connection
            .find_pull_request_by_head(&owner, &repo_name, &info.head_branch)
            .await
        {
            Ok(Some((pr_id, number, is_draft))) => {
                match connection
                    .update_pull_request(
                        &pr_id,
                        Some(&info.title),
                        Some(&info.body),
                        Some(effective_base_branch),
                    )
                    .await
                {
                    Ok((_, _)) => {
                        if is_draft != desired_draft {
                            let r = if desired_draft {
                                connection.convert_pull_request_to_draft(&pr_id).await
                            } else {
                                connection.mark_pull_request_ready_for_review(&pr_id).await
                            };
                            match r {
                                Ok((_, _, new_is_draft)) => eprintln!(
                                    "Updated PR #{}: {} (base: {}, draft: {})",
                                    number, info.head_branch, effective_base_branch, new_is_draft
                                ),
                                Err(e) => eprintln!(
                                    "Updated PR #{}: {} (base: {}), but failed to update draft status: {}",
                                    number, info.head_branch, effective_base_branch, e
                                ),
                            }
                        } else {
                            eprintln!(
                                "Updated PR #{}: {} (base: {}, draft: {})",
                                number, info.head_branch, effective_base_branch, is_draft
                            );
                        }
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        eprintln!(
                            "Failed to update PR #{} {}: {}",
                            number, info.head_branch, msg
                        );
                    }
                }
            }
            Ok(None) => {
                match connection
                    .create_pull_request(
                        &repository_id,
                        effective_base_branch,
                        &info.head_branch,
                        &info.title,
                        &info.body,
                        desired_draft,
                    )
                    .await
                {
                    Ok((_, number)) => eprintln!(
                        "Created PR #{}: {} → {} (draft: {})",
                        number, info.head_branch, effective_base_branch, desired_draft
                    ),
                    Err(e) => {
                        let msg = e.to_string();
                        eprintln!(
                            "Failed to create PR {} → {}: {}",
                            info.head_branch, effective_base_branch, msg
                        );
                    }
                }
            }
            Err(e) => eprintln!("Failed to look up PR for {}: {}", info.head_branch, e),
        }
    }

    Ok(())
}

/// Write PR comments into refs/josh/changes. Shared by both sync paths.
fn write_pr_comments(
    repo: &git2::Repository,
    change: &josh_changes::Change,
    pr_data: &josh_github_graphql::operations::pull_request::PrData,
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
        )?;
        // Record the GitHub node ID so this comment is tracked as "already posted".
        if let Some(change_id) = change.id() {
            josh_changes::store_github_id(repo, change_id, &hash, &comment.id)?;
        }
        id_map.insert(comment.id.clone(), hash);
    }

    Ok(pr_data.comments.len())
}

/// Sync GitHub PR comments for a single change into refs/josh/changes.
/// Returns the number of comments synced.
pub async fn sync_change_comments(
    connection: &GithubApiConnection,
    owner: &str,
    repo_name: &str,
    repo: &git2::Repository,
    change: &josh_changes::Change,
    head_ref: &str,
) -> anyhow::Result<usize> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(0),
    };

    let pr = match connection
        .find_pull_request_by_head(owner, repo_name, head_ref)
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
    josh_changes::store_pr_data(repo, change_id, &json)?;

    write_pr_comments(repo, change, &pr_data)
}

/// Sync GitHub PR comments for a change identified directly by PR number.
pub async fn sync_change_comments_by_pr_number(
    connection: &GithubApiConnection,
    owner: &str,
    repo_name: &str,
    repo: &git2::Repository,
    change: &josh_changes::Change,
    pr_number: i64,
) -> anyhow::Result<usize> {
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(0),
    };

    let pr_data = connection
        .get_pr_comments(owner, repo_name, pr_number)
        .await?;
    let json = serde_json::to_string(&pr_data)?;
    josh_changes::store_pr_data(repo, change_id, &json)?;

    write_pr_comments(repo, change, &pr_data)
}

/// Post local comments (those without a `github_id`) to a GitHub PR.
/// Returns the number of comments successfully posted.
pub async fn post_local_comments(
    connection: &GithubApiConnection,
    repo: &git2::Repository,
    change_id: &str,
    pr_node_id: &str,
) -> anyhow::Result<usize> {
    let comments = josh_changes::read_comments(repo, change_id)?;
    if comments.is_empty() {
        return Ok(0);
    }

    let github_ids = josh_changes::read_github_ids(repo, change_id)?;

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

            josh_changes::store_github_id(repo, change_id, &comment.id, &github_id)?;
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
                josh_changes::store_github_id(repo, change_id, &comment.id, &github_id)?;
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
pub async fn post_local_votes(
    connection: &GithubApiConnection,
    repo: &git2::Repository,
    change_id: &str,
    pr_node_id: &str,
    commit_oid: &str,
) -> anyhow::Result<usize> {
    let votes = josh_changes::list_votes(repo, change_id)?;
    if votes.is_empty() {
        return Ok(0);
    }

    let tracked = josh_changes::read_github_vote_ids(repo, change_id)?;

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

        josh_changes::store_github_vote_id(repo, change_id, user, vote_data)?;
        posted += 1;
    }

    Ok(posted)
}
