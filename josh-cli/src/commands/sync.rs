use anyhow::{Context, anyhow};

use josh_core::git::normalize_repo_path;

use crate::config::read_remote_config;
use crate::forge::Forge;
use crate::forge::github;
use serde_json;

/// Arguments for `josh changes sync`.
#[derive(Debug, clap::Parser)]
pub struct SyncArgs {
    /// Josh remote name (default: origin).
    #[arg()]
    pub remote: Option<String>,

    /// Discard existing refs/josh/changes before syncing.
    #[arg(long = "clean")]
    pub clean: bool,

    /// Skip GitHub comment syncing; only update refs/josh/changes locally.
    #[arg(long = "local")]
    pub local: bool,

    /// Push local comments that haven't been posted to GitHub yet.
    #[arg(long = "push")]
    pub push: bool,
}

pub fn handle_sync(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let head = repo.head()?.peel_to_commit()?;
    let branch = repo.head()?.shorthand().map(|s| s.to_string());

    let base_oid = branch
        .as_ref()
        .and_then(|b| {
            repo.find_reference(&format!("refs/remotes/origin/{}", b))
                .ok()
                .and_then(|r| r.peel_to_commit().ok())
                .map(|c| c.id())
        })
        .unwrap_or(git2::Oid::zero());

    if args.clean {
        if let Ok(mut r) = repo.find_reference("refs/josh/changes") {
            r.delete()?;
        }
    }

    if !args.local {
        let repo_path = normalize_repo_path(repo.path());

        let remote_config =
            read_remote_config(&repo_path, args.remote.as_deref().unwrap_or("origin"))
                .with_context(|| {
                    format!(
                        "Failed to read remote config for '{}'",
                        args.remote.as_deref().unwrap_or("origin")
                    )
                })?;

        if remote_config.forge != Some(Forge::Github) {
            return Err(anyhow!("sync is only supported for GitHub remotes"));
        }

        let (owner, repo_name) = josh_github_changes::repo::parse_owner_repo(&remote_config.url)?;

        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let api = github::make_api_connection()
                .await
                .with_context(|| github::api_connection_hint())?;

            let prs = api.list_open_pull_requests(&owner, &repo_name).await?;
            println!("Found {} open PRs on GitHub.", prs.len());

            if prs.is_empty() {
                return Ok(());
            }

            // Collect all unique OIDs: PR head commits + target branch tips.
            let mut oids: Vec<String> = Vec::new();
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for pr in &prs {
                if seen.insert(&pr.head_oid) {
                    oids.push(pr.head_oid.clone());
                }
                if seen.insert(&pr.base_ref_oid) {
                    oids.push(pr.base_ref_oid.clone());
                }
            }

            // Fetch all needed objects by SHA from GitHub.
            let github_url = format!("https://github.com/{}/{}", owner, repo_name);
            if !oids.is_empty() {
                let mut fetch_args: Vec<&str> = Vec::with_capacity(3 + oids.len());
                fetch_args.push("fetch");
                fetch_args.push(&github_url);
                fetch_args.push("--no-tags");
                let oid_strs: Vec<String> = oids.iter().map(|o| o.to_string()).collect();
                for oid in &oid_strs {
                    fetch_args.push(oid);
                }
                josh_core::git::spawn_git_command(repo.path(), &fetch_args, &[])
                    .with_context(|| "Failed to fetch objects from GitHub")?;
            }

            // Refresh ODB so git2 sees the newly fetched objects.
            repo.odb()?.refresh()?;

            // Collect target branches from @changes/... head refs and fetch their tips.
            let mut target_branch_shas: std::collections::HashMap<String, git2::Oid> =
                std::collections::HashMap::new();
            {
                let mut seen_targets: std::collections::HashSet<&str> =
                    std::collections::HashSet::new();
                for pr in &prs {
                    if let Some(target) = parse_changes_target(&pr.head_ref_name) {
                        if seen_targets.insert(target) {
                            let refspec = format!("refs/heads/{}", target);
                            let fetch_args: Vec<&str> =
                                vec!["fetch", &github_url, "--no-tags", &refspec];
                            josh_core::git::spawn_git_command(repo.path(), &fetch_args, &[])
                                .with_context(|| {
                                    format!("Failed to fetch target branch {}", target)
                                })?;
                            let output = std::process::Command::new("git")
                                .args(["rev-parse", "FETCH_HEAD"])
                                .current_dir(repo.path())
                                .output()
                                .with_context(
                                    || "Failed to resolve FETCH_HEAD after target branch fetch",
                                )?;
                            let sha_str = String::from_utf8(output.stdout)?.trim().to_string();
                            let oid = git2::Oid::from_str(&sha_str)?;
                            target_branch_shas.insert(target.to_string(), oid);
                        }
                    }
                }
            }
            if !target_branch_shas.is_empty() {
                repo.odb()?.refresh()?;
            }

            let mut total_comments = 0usize;
            let mut synced = 0usize;
            let mut skipped = 0usize;

            for pr in &prs {
                let (existing_change_id, _) =
                    josh_changes::parse_change_meta(&pr.head_commit_message);

                let head_oid = match git2::Oid::from_str(&pr.head_oid) {
                    Ok(o) => o,
                    Err(e) => {
                        eprintln!("PR #{}: bad head OID: {}", pr.number, e);
                        skipped += 1;
                        continue;
                    }
                };

                let pr_head = match repo.find_commit(head_oid) {
                    Ok(c) => c,
                    Err(_) => {
                        eprintln!(
                            "PR #{}: head commit {} not available from GitHub — skipping",
                            pr.number, pr.head_oid
                        );
                        skipped += 1;
                        continue;
                    }
                };

                let base_oid = match git2::Oid::from_str(&pr.base_ref_oid) {
                    Ok(o) => o,
                    Err(e) => {
                        eprintln!("PR #{}: bad base OID: {}", pr.number, e);
                        skipped += 1;
                        continue;
                    }
                };

                let target = match repo.find_commit(base_oid) {
                    Ok(c) => c,
                    Err(_) => {
                        eprintln!(
                            "PR #{}: base commit {} not available from GitHub — skipping",
                            pr.number, pr.base_ref_oid
                        );
                        skipped += 1;
                        continue;
                    }
                };

                if let Some(ref cid) = existing_change_id {
                    println!("PR #{}: head commit has change-id '{}'", pr.number, cid);
                } else {
                    println!(
                        "PR #{}{}: creating synthetic merge commit",
                        pr.number,
                        if pr.title.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", &pr.title)
                        }
                    );
                }

                let result = (|| -> anyhow::Result<(josh_changes::Change, i64)> {
                    let change = if existing_change_id.is_some() {
                        let mut change = josh_changes::Change::new(repo, &pr_head);
                        let base = match parse_changes_target(&pr.head_ref_name)
                            .and_then(|t| target_branch_shas.get(t))
                        {
                            Some(tip) => repo.merge_base(*tip, pr_head.id())?,
                            None => repo.merge_base(target.id(), pr_head.id())?,
                        };
                        change.set_base(base);
                        change
                    } else {
                        let change_id = format!("{}/{}/pull/{}", owner, repo_name, pr.number);
                        let mut message = pr.title.clone();
                        if !pr.body.is_empty() {
                            message.push_str("\n\n");
                            message.push_str(&pr.body);
                        }
                        message.push_str(&format!("\n\nChange-Id: {}\n", change_id));

                        let merge_oid = josh_changes::create_synthetic_merge_commit(
                            repo, &pr_head, &target, &message,
                        )?;

                        let merge = repo.find_commit(merge_oid)?;
                        let mut change = josh_changes::Change::new(repo, &merge);
                        change.set_base(target.id());
                        change
                    };

                    josh_changes::store_diff_data(repo, &change)?;
                    Ok((change, pr.number))
                })();

                match result {
                    Ok((change, pr_number)) => {
                        match josh_github_changes::sync_change_comments_by_pr_number(
                            &api, &owner, &repo_name, repo, &change, pr_number,
                        )
                        .await
                        {
                            Ok(n) => {
                                total_comments += n;
                                synced += 1;
                                println!("  PR #{}: synced {} comments", pr.number, n);
                            }
                            Err(e) => {
                                eprintln!("PR #{}: {} — skipping", pr.number, e);
                                skipped += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("PR #{}: {} — skipping", pr.number, e);
                        skipped += 1;
                    }
                }
            }

            println!(
                "Synced {} comments across {} PRs ({} skipped).",
                total_comments, synced, skipped
            );

            // Build the set of open change IDs from the PRs we just synced.
            let open_change_ids: std::collections::HashSet<String> = prs
                .iter()
                .map(|pr| {
                    let (existing_id, _) = josh_changes::parse_change_meta(&pr.head_commit_message);
                    existing_id
                        .unwrap_or_else(|| format!("{}/{}/pull/{}", owner, repo_name, pr.number))
                })
                .collect();

            let all_changes = josh_changes::list_changes(repo)?;
            let mut cleaned = 0usize;

            for change in &all_changes {
                let change_id = match change.id() {
                    Some(id) => id,
                    None => continue,
                };

                if open_change_ids.contains(change_id) {
                    continue;
                }

                // Determine the PR number for this change.
                let pr_number: i64 =
                    match parse_pr_number_from_change_id(change_id, &owner, &repo_name) {
                        Some(n) => n,
                        None => {
                            // Custom Change-Id; try reading stored PR data.
                            match josh_changes::read_pr_data(repo, change_id) {
                                Ok(Some(json)) => {
                                    match serde_json::from_str::<serde_json::Value>(&json) {
                                        Ok(v) => match v.get("number").and_then(|n| n.as_i64()) {
                                            Some(n) => n,
                                            None => {
                                                eprintln!(
                                                    "  Change '{}': no PR number in stored data \
                                                 -- skipping",
                                                    change_id
                                                );
                                                continue;
                                            }
                                        },
                                        Err(e) => {
                                            eprintln!(
                                                "  Change '{}': invalid stored PR data: {} \
                                             -- skipping",
                                                change_id, e
                                            );
                                            continue;
                                        }
                                    }
                                }
                                Ok(None) => {
                                    // Purely local change with no PR data at all.
                                    continue;
                                }
                                Err(e) => {
                                    eprintln!(
                                        "  Change '{}': failed to read PR data: {} -- skipping",
                                        change_id, e
                                    );
                                    continue;
                                }
                            }
                        }
                    };

                // Fetch the current PR data from GitHub.
                let pr_data = match api.get_pr_comments(&owner, &repo_name, pr_number).await {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!(
                            "  Change '{}' (PR #{}): failed to fetch PR data: {} -- skipping",
                            change_id, pr_number, e
                        );
                        continue;
                    }
                };

                // Guard: if the PR is still open, do not delete the change.
                if pr_data.state == "OPEN" {
                    // Record the current state even if unexpectedly open.
                    let json = serde_json::to_string(&pr_data)?;
                    josh_changes::store_pr_data(repo, change_id, &json)?;
                    eprintln!(
                        "  Change '{}' (PR #{}): unexpectedly still OPEN on GitHub \
                         -- skipping deletion",
                        change_id, pr_number
                    );
                    continue;
                }

                // Commit 1: store the updated PR data (final CLOSED/MERGED state).
                let json = serde_json::to_string(&pr_data)?;
                if let Err(e) = josh_changes::store_pr_data(repo, change_id, &json) {
                    eprintln!(
                        "  Change '{}' (PR #{}): failed to store updated PR data: {} \
                         -- skipping deletion",
                        change_id, pr_number, e
                    );
                    continue;
                }

                // Commit 2: delete the change from refs/josh/changes.
                if let Err(e) = josh_changes::delete_change(repo, change_id) {
                    eprintln!(
                        "  Change '{}' (PR #{}): failed to delete: {}",
                        change_id, pr_number, e
                    );
                } else {
                    println!(
                        "  Cleaned up '{}' (PR #{}: {})",
                        change_id, pr_number, pr_data.state
                    );
                    cleaned += 1;
                }
            }

            if cleaned > 0 {
                println!("Cleaned up {} closed/merged changes.", cleaned);
            }

            if args.push {
                let mut total_posted = 0usize;
                for pr in &prs {
                    let head_oid = match git2::Oid::from_str(&pr.head_oid) {
                        Ok(o) => o,
                        Err(e) => {
                            eprintln!("PR #{}: bad head OID for comment push: {}", pr.number, e);
                            continue;
                        }
                    };
                    let pr_head = match repo.find_commit(head_oid) {
                        Ok(c) => c,
                        Err(_) => {
                            eprintln!(
                                "PR #{}: head commit not available — skipping comment push",
                                pr.number
                            );
                            continue;
                        }
                    };
                    let base_oid = match git2::Oid::from_str(&pr.base_ref_oid) {
                        Ok(o) => o,
                        Err(e) => {
                            eprintln!("PR #{}: bad base OID for comment push: {}", pr.number, e);
                            continue;
                        }
                    };
                    let target = match repo.find_commit(base_oid) {
                        Ok(c) => c,
                        Err(_) => {
                            eprintln!(
                                "PR #{}: base commit not available — skipping comment push",
                                pr.number
                            );
                            continue;
                        }
                    };
                    let mut change = josh_changes::Change::new(repo, &pr_head);
                    let base = repo.merge_base(target.id(), pr_head.id())?;
                    change.set_base(base);

                    match api
                        .find_pull_request_by_head(&owner, &repo_name, &pr.head_ref_name)
                        .await
                    {
                        Ok(Some((pr_node_id, _, _))) => {
                            match josh_github_changes::post_local_comments(
                                &api,
                                repo,
                                &change,
                                &pr_node_id,
                            )
                            .await
                            {
                                Ok(n) => {
                                    total_posted += n;
                                    if n > 0 {
                                        println!(
                                            "  PR #{}: posted {} local comments",
                                            pr.number, n
                                        );
                                    }
                                }
                                Err(e) => {
                                    eprintln!(
                                        "  PR #{}: failed to post comments: {}",
                                        pr.number, e
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            eprintln!(
                                "  No open PR found for {} — skipping comment push",
                                pr.head_ref_name
                            );
                        }
                        Err(e) => {
                            eprintln!("  Failed to look up PR for {}: {}", pr.head_ref_name, e);
                        }
                    }
                }
                println!("Posted {} local comments to GitHub.", total_posted);
            }

            Ok::<_, anyhow::Error>(())
        })?;
    } else {
        let changes = josh_changes::sync_changes(repo, head.id(), base_oid)?;
        if changes.is_empty() {
            println!("No local changes found.");
            return Ok(());
        }
    }

    Ok(())
}

/// Extract the target branch name from a `@changes/<target>/<author>/<change-id>` ref name.
fn parse_changes_target(head_ref_name: &str) -> Option<&str> {
    let name = head_ref_name
        .strip_prefix("refs/heads/@changes/")
        .or_else(|| head_ref_name.strip_prefix("@changes/"))?;
    let mut end = 0;
    for part in name.split('/') {
        if part.contains('@') {
            return Some(&name[..end].trim_end_matches('/'));
        }
        end += part.len() + 1;
    }
    None
}

/// Extract the PR number from a synthetic change ID of the form `{owner}/{repo}/pull/{N}`.
fn parse_pr_number_from_change_id(change_id: &str, owner: &str, repo: &str) -> Option<i64> {
    let prefix = format!("{}/{}/pull/", owner, repo);
    change_id.strip_prefix(&prefix)?.parse().ok()
}
