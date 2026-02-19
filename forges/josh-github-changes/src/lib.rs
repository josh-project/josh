mod repo;

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
pub fn collect_pr_infos(
    repo: &git2::Repository,
    to_push: &[(String, git2::Oid, String)],
) -> Vec<PrInfo> {
    fn branch_name(refname: &str) -> &str {
        refname.strip_prefix("refs/heads/").unwrap_or(refname)
    }
    let mut by_id: HashMap<
        String,
        (
            Option<String>,
            Option<String>,
            Option<git2::Oid>,
            Option<git2::Oid>,
        ),
    > = HashMap::new();
    for (refname, oid, id) in to_push {
        let branch = branch_name(refname).to_string();
        if refname.contains("@changes") {
            let entry = by_id.entry(id.clone()).or_default();
            entry.0 = Some(branch);
            entry.2 = Some(*oid);
        } else if refname.contains("@base") {
            let entry = by_id.entry(id.clone()).or_default();
            entry.1 = Some(branch);
            entry.3 = Some(*oid);
        }
    }
    by_id
        .into_iter()
        .filter_map(|(_, (head, base, head_oid, base_oid))| {
            let (head, base, head_oid, base_oid) = (head?, base?, head_oid?, base_oid?);
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
