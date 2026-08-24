//! Creating and updating GitHub pull requests for stacked changes, and
//! reading back the stored per-change PR data.

use std::collections::HashMap;

use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::pull_request::PrData;

use crate::display::pr_link;
use crate::layout::{GithubChangesRefData, GITHUB_PR_DATA_PATH};

/// Store the PR data for a change: a sparse `GithubChangesRefData` carrying
/// only the `gh/<change-id>` entry, merged into the ref.
pub fn store_pr_data(
    transaction: &josh_core::cache::Transaction,
    change_id: &str,
    data: &PrData,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<()> {
    let sparse = GithubChangesRefData {
        gh: [(change_id.to_string(), data.clone())].into(),
        ..Default::default()
    };
    josh_changes::write_filtered(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_PR_DATA_PATH),
        &sparse,
        None,
        None,
    )?;
    Ok(())
}

/// Read the stored PR data for a change, if present. Fails when the stored
/// tree does not decode (`sync --clean` rebuilds the ref).
pub fn read_pr_data(
    transaction: &josh_core::cache::Transaction,
    change_id: &str,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<Option<PrData>> {
    let Some(mut data) = josh_changes::read_filtered::<GithubChangesRefData>(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_PR_DATA_PATH),
    )?
    else {
        return Ok(None);
    };
    Ok(data.gh.remove(change_id))
}

#[derive(Debug)]
pub struct PrInfo {
    pub head_branch: String,
    pub base_branch: String,
    pub base_oid: gix_hash::ObjectId,
    pub title: String,
    pub body: String,
}

/// Collect PR info from a set of refs to push.
/// Uses the @base ref for each change as the base branch. Title and body come from the head commit message.
pub fn collect_pr_infos(
    transaction: &josh_core::cache::Transaction,
    to_push: &[josh_changes::PushRef],
) -> Vec<PrInfo> {
    #[derive(Default)]
    struct ByIdEntry {
        head_branch: Option<String>,
        base_branch: Option<String>,
        head_oid: Option<gix_hash::ObjectId>,
        base_oid: Option<gix_hash::ObjectId>,
    }

    fn branch_name(refname: &str) -> &str {
        refname.strip_prefix("refs/heads/").unwrap_or(refname)
    }

    let mut by_change_id: HashMap<String, ByIdEntry> = HashMap::new();
    for push_ref in to_push {
        let branch = branch_name(&push_ref.ref_name).to_string();
        match josh_changes::StackedRef::parse(&push_ref.ref_name) {
            Some(josh_changes::StackedRef::ChangeRef(josh_changes::StackedChangeRef::Change {
                ..
            })) => {
                let entry = by_change_id.entry(push_ref.change_id.clone()).or_default();
                entry.head_branch = Some(branch);
                entry.head_oid = Some(push_ref.oid);
            }
            Some(josh_changes::StackedRef::ChangeRef(josh_changes::StackedChangeRef::Base {
                ..
            })) => {
                let entry = by_change_id.entry(push_ref.change_id.clone()).or_default();
                entry.base_branch = Some(branch);
                entry.base_oid = Some(push_ref.oid);
            }
            _ => {}
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
            let odb = transaction.odb();
            let commit = josh_core::objects::CommitData::read(odb, head_oid).ok()?;
            let raw_message = commit
                .message()
                .ok()
                .and_then(|m| std::str::from_utf8(m).ok())?;
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

/// The base branch, draft state, and head ref chosen for a single PR.
#[derive(Debug, PartialEq)]
struct PrPlan {
    base_branch: String,
    draft: bool,
    head_ref: String,
}

/// Decide how a change's PR should be targeted.
///
/// `default_branch` is the target repo's `(name, tip_oid)`, if known. `fork_owner`
/// is set when the head branch lives in a fork, in which case the head is
/// namespaced as `fork_owner:branch` and the PR must base on the target's
/// default branch (a PR base cannot live in a fork). Returns `None` when a
/// cross-fork PR is requested but the target default branch is unknown.
fn plan_pr(
    info: &PrInfo,
    default_branch: Option<&(String, String)>,
    fork_owner: Option<&str>,
) -> Option<PrPlan> {
    let head_ref = match fork_owner {
        Some(fork_owner) => format!("{}:{}", fork_owner, info.head_branch),
        None => info.head_branch.clone(),
    };

    // Fork-mode PRs can only base on a branch that exists in the target repo,
    // so they always base on its default branch; the change is a draft while its
    // base still lags the default branch tip (i.e. it depends on unmerged work).
    if fork_owner.is_some() {
        let (default_name, default_oid) = default_branch?;
        return Some(PrPlan {
            base_branch: default_name.clone(),
            draft: info.base_oid.to_string() != *default_oid,
            head_ref,
        });
    }

    // Same-repo PRs may base on the synthetic `@base/…` branch pushed alongside.
    let base_branch = match default_branch {
        Some((default_name, default_oid)) if info.base_oid.to_string() == *default_oid => {
            default_name.clone()
        }
        _ => info.base_branch.clone(),
    };
    let draft = match default_branch {
        Some((default_name, _)) => base_branch != *default_name,
        None => base_branch == info.base_branch,
    };
    Some(PrPlan {
        base_branch,
        draft,
        head_ref,
    })
}

/// Create or update GitHub PRs for a set of changes.
///
/// `url` is the PR target (upstream). When `fork_url` is `Some`, the change
/// branches live in that fork and PRs are opened with a cross-fork head
/// (`fork_owner:branch`). Because a PR's base branch must live in the target
/// repo, fork-mode PRs always base on the target's default branch; a change
/// whose base is not yet the default branch tip (i.e. it still depends on
/// unmerged changes) is opened as a draft.
pub async fn create_or_update_prs(
    connection: &GithubApiConnection,
    url: &str,
    fork_url: Option<&str>,
    pr_infos: &[PrInfo],
    dry_run: bool,
) -> anyhow::Result<()> {
    let (owner, repo_name) = crate::repo::parse_owner_repo(url)?;

    let fork = fork_url.map(crate::repo::parse_owner_repo).transpose()?;
    let fork_owner = fork.as_ref().map(|(o, _)| o.as_str());

    let repository_id = connection.get_repo_id(&owner, &repo_name).await?;
    let default_branch = connection.get_default_branch(&owner, &repo_name).await?;

    for info in pr_infos {
        let plan = match plan_pr(info, default_branch.as_ref(), fork_owner) {
            Some(plan) => plan,
            None => {
                eprintln!(
                    "Skipping PR for {}: target default branch is unknown, \
                     cannot open a cross-fork PR",
                    info.head_branch
                );
                continue;
            }
        };
        let effective_base_branch = plan.base_branch.as_str();
        let desired_draft = plan.draft;
        let head_ref_for_create = plan.head_ref;

        if dry_run {
            match connection
                .find_pull_request_by_head(&owner, &repo_name, &info.head_branch, fork_owner)
                .await
            {
                Ok(Some((_, number, is_draft))) => eprintln!(
                    "Would update PR #{}: {} → {} (draft: {} → {})",
                    number, head_ref_for_create, effective_base_branch, is_draft, desired_draft
                ),
                Ok(None) => eprintln!(
                    "Would create PR: {} → {} (draft: {})",
                    head_ref_for_create, effective_base_branch, desired_draft
                ),
                Err(e) => eprintln!("Failed to look up PR for {}: {}", info.head_branch, e),
            }
            continue;
        }

        match connection
            .find_pull_request_by_head(&owner, &repo_name, &info.head_branch, fork_owner)
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
                                    "Updated {}: {} (base: {}, draft: {})",
                                    pr_link(url, number),
                                    info.head_branch,
                                    effective_base_branch,
                                    new_is_draft
                                ),
                                Err(e) => eprintln!(
                                    "Updated PR #{}: {} (base: {}), but failed to update draft status: {}",
                                    number, info.head_branch, effective_base_branch, e
                                ),
                            }
                        } else {
                            eprintln!(
                                "Updated {}: {} (base: {}, draft: {})",
                                pr_link(url, number),
                                info.head_branch,
                                effective_base_branch,
                                is_draft
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
                        &head_ref_for_create,
                        &info.title,
                        &info.body,
                        desired_draft,
                    )
                    .await
                {
                    Ok((_, number)) => eprintln!(
                        "Created {}: {} → {} (draft: {})",
                        pr_link(url, number),
                        info.head_branch,
                        effective_base_branch,
                        desired_draft
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const TIP: &str = "1111111111111111111111111111111111111111";
    const OTHER: &str = "2222222222222222222222222222222222222222";

    fn pr_info(base_oid: &str) -> PrInfo {
        PrInfo {
            head_branch: "@changes/main/a@b.com/feature".to_string(),
            base_branch: "@base/main/a@b.com/feature".to_string(),
            base_oid: gix_hash::ObjectId::from_str(base_oid).unwrap(),
            title: "t".to_string(),
            body: "b".to_string(),
        }
    }

    fn default_branch() -> (String, String) {
        ("main".to_string(), TIP.to_string())
    }

    #[test]
    fn fork_change_on_default_is_ready_against_default() {
        let info = pr_info(TIP);
        let plan = plan_pr(&info, Some(&default_branch()), Some("forkowner")).unwrap();
        assert_eq!(plan.base_branch, "main");
        assert!(!plan.draft);
        assert_eq!(plan.head_ref, "forkowner:@changes/main/a@b.com/feature");
    }

    #[test]
    fn fork_dependent_change_is_draft_against_default() {
        let info = pr_info(OTHER);
        let plan = plan_pr(&info, Some(&default_branch()), Some("forkowner")).unwrap();
        assert_eq!(plan.base_branch, "main");
        assert!(plan.draft);
        assert_eq!(plan.head_ref, "forkowner:@changes/main/a@b.com/feature");
    }

    #[test]
    fn fork_without_known_default_is_skipped() {
        let info = pr_info(TIP);
        assert_eq!(plan_pr(&info, None, Some("forkowner")), None);
    }

    #[test]
    fn same_repo_bottom_change_bases_on_default() {
        let info = pr_info(TIP);
        let plan = plan_pr(&info, Some(&default_branch()), None).unwrap();
        assert_eq!(plan.base_branch, "main");
        assert!(!plan.draft);
        assert_eq!(plan.head_ref, "@changes/main/a@b.com/feature");
    }

    #[test]
    fn same_repo_stacked_change_bases_on_synthetic_branch_as_draft() {
        let info = pr_info(OTHER);
        let plan = plan_pr(&info, Some(&default_branch()), None).unwrap();
        assert_eq!(plan.base_branch, "@base/main/a@b.com/feature");
        assert!(plan.draft);
        assert_eq!(plan.head_ref, "@changes/main/a@b.com/feature");
    }
}
