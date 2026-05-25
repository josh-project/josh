use anyhow::{Context, anyhow};

use josh_core::git::normalize_repo_path;

use crate::config::read_remote_config;
use crate::forge::Forge;
use crate::forge::github;

/// Arguments for `josh changes sync`.
#[derive(Debug, clap::Parser)]
pub struct SyncArgs {
    /// Josh remote name (default: origin).
    #[arg()]
    pub remote: Option<String>,
}

pub fn handle_sync(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());
    let remote_name = args.remote.as_deref().unwrap_or("origin");

    let remote_config = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

    if remote_config.forge != Some(Forge::Github) {
        return Err(anyhow!("sync is only supported for GitHub remotes"));
    }

    let head = repo.head()?.peel_to_commit()?;
    let branch = repo.head()?.shorthand().map(|s| s.to_string());

    let base = if let Some(ref name) = branch {
        let remote_ref = format!("refs/remotes/origin/{}", name);
        repo.find_reference(&remote_ref).ok().map(|_| name.clone())
    } else {
        None
    };
    let base_name = base.clone().unwrap_or_else(|| "master".to_string());
    let base_oid = branch
        .as_ref()
        .and_then(|b| {
            repo.find_reference(&format!("refs/remotes/origin/{}", b))
                .ok()
                .and_then(|r| r.peel_to_commit().ok())
                .map(|c| c.id())
        })
        .unwrap_or(git2::Oid::zero());

    let git_config = repo.config()?;
    let email = git_config.get_string("user.email").unwrap_or_default();

    let changes = josh_changes::list_changes(repo, head.id(), base_oid)?;
    if changes.is_empty() {
        println!("No local changes found.");
        return Ok(());
    }

    let (owner, repo_name) = josh_github_changes::repo::parse_owner_repo(&remote_config.url)?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let api = github::make_api_connection()
            .await
            .with_context(|| github::api_connection_hint())?;

        for change in &changes {
            let change_id = match change.id() {
                Some(id) => id,
                None => continue,
            };

            let head_ref = format!("@changes/{}/{}/{}", base_name, email, change_id);

            let pr = match api
                .find_pull_request_by_head(&owner, &repo_name, &head_ref)
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
                    continue;
                }
            };

            let pr_data = api.get_pr_comments(&owner, &repo_name, pr).await?;

            // Write each comment and review, building a GitHub ID -> our hash mapping.
            let mut id_map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();

            for comment in &pr_data.comments {
                let location = comment.path.as_ref().zip(comment.line).map(|(path, line)| {
                    josh_changes::Location {
                        path: path.clone(),
                        start_line: line as u32,
                        end_line: line as u32,
                        start_col: 1,
                        end_col: u32::MAX,
                    }
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

                let diff_id = comment
                    .commit_oid
                    .as_ref()
                    .and_then(|oid| git2::Oid::from_str(oid).ok())
                    .and_then(|oid| josh_changes::diff_id(repo, oid).ok());

                let hash = josh_changes::write_comment_with_diff(
                    repo,
                    change,
                    &meta,
                    Some(&comment.author),
                    Some(&comment.timestamp),
                    diff_id.as_deref(),
                )?;
                id_map.insert(comment.id.clone(), hash);
            }
        }

        Ok::<_, anyhow::Error>(())
    })?;

    println!("Sync complete.");
    Ok(())
}
