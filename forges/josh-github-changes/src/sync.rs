use anyhow::{anyhow, Context};

use josh_core::git::normalize_repo_path;

use crate::connection::{api_connection_hint, make_api_connection};
use crate::repo::parse_owner_repo;

#[derive(Debug, Default, Clone, Copy)]
pub struct SyncOptions {
    /// Discard existing refs/josh/changes (for the resolved scope kind) before syncing.
    pub clean: bool,
    /// Push outbox comments and votes to GitHub (Remote scope only).
    pub push: bool,
}

/// Totals collected during a sync run. Returned to the caller for any final
/// reporting; per-PR progress lines are still printed inline to stdout for
/// CLI parity.
#[derive(Debug, Default, Clone, Copy)]
pub struct SyncReport {
    pub local_changes: usize,
    pub synced: usize,
    pub skipped: usize,
    pub total_comments: usize,
    pub cleaned: usize,
    pub total_posted: usize,
    pub total_votes_posted: usize,
}

/// Dispatch to the right sync backend based on the scope.
///
/// For `Local`, runs `josh_changes::sync_local`. For `Remote`, reads the
/// remote's josh config, requires `forge = github`, and calls
/// `sync_from_github`.
pub async fn sync(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    scope: &josh_changes::ChangesRef,
    opts: SyncOptions,
) -> anyhow::Result<SyncReport> {
    match scope {
        josh_changes::ChangesRef::Local { branch } => {
            if opts.push {
                return Err(anyhow!(
                    "--push requires --remote <name>; the Local ref has no posting target"
                ));
            }
            if opts.clean {
                // Match handle_sync's prior behavior: delete every Local ref,
                // not just the resolved scope's.
                let to_delete: Vec<josh_changes::ChangesRef> =
                    josh_changes::all_changes_refs(repo)?
                        .into_iter()
                        .filter(|s| matches!(s, josh_changes::ChangesRef::Local { .. }))
                        .collect();
                for s in to_delete {
                    if let Ok(mut r) = repo.find_reference(&s.ref_name()) {
                        r.delete()?;
                    }
                }
            }
            let changes = josh_changes::sync_local(repo, transaction, branch)?;
            if changes.is_empty() {
                println!("No local changes found.");
            }
            Ok(SyncReport {
                local_changes: changes.len(),
                ..Default::default()
            })
        }
        josh_changes::ChangesRef::Remote { remote, branch } => {
            let repo_path = normalize_repo_path(repo.path());
            let remote_config = josh_changes::remote_config::read_remote_config(&repo_path, remote)
                .with_context(|| format!("Failed to read remote config for '{}'", remote))?;

            if remote_config.forge != Some(josh_changes::remote_config::Forge::Github) {
                return Err(anyhow!("sync is only supported for GitHub remotes"));
            }

            sync_from_github(repo, transaction, remote, branch, &remote_config.url, opts).await
        }
    }
}

/// Run a GitHub-backed sync against `remote_name` for changes targeting
/// `branch`. Fetches open PRs, builds/updates Change objects, syncs comments,
/// cleans up merged/closed changes, and optionally pushes outbox comments and
/// votes when `opts.push` is set.
pub async fn sync_from_github(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
    branch: &str,
    github_url: &str,
    opts: SyncOptions,
) -> anyhow::Result<SyncReport> {
    let _ = transaction;
    let _ = branch;

    if opts.clean {
        // Delete every changes ref under this remote.
        let mut to_delete: Vec<josh_changes::ChangesRef> = Vec::new();
        for scope in josh_changes::all_changes_refs(repo)? {
            if let josh_changes::ChangesRef::Remote { remote, .. } = &scope {
                if remote == remote_name {
                    to_delete.push(scope);
                }
            }
        }
        for scope in to_delete {
            if let Ok(mut r) = repo.find_reference(&scope.ref_name()) {
                r.delete()?;
            }
        }
    }

    let (owner, repo_name) = parse_owner_repo(github_url)?;

    let api = make_api_connection()
        .await
        .with_context(api_connection_hint)?;

    let prs = api.list_open_pull_requests(&owner, &repo_name).await?;
    println!("Found {} open PRs on GitHub.", prs.len());

    let mut report = SyncReport::default();

    if prs.is_empty() {
        return Ok(report);
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
    let github_url_str = format!("https://github.com/{}/{}", owner, repo_name);
    if !oids.is_empty() {
        let mut fetch_args: Vec<&str> = Vec::with_capacity(3 + oids.len());
        fetch_args.push("fetch");
        fetch_args.push(&github_url_str);
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
        let mut seen_targets: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for pr in &prs {
            if let Some(target) = parse_changes_target(&pr.head_ref_name) {
                if seen_targets.insert(target) {
                    let refspec = format!("refs/heads/{}", target);
                    let fetch_args: Vec<&str> =
                        vec!["fetch", &github_url_str, "--no-tags", &refspec];
                    josh_core::git::spawn_git_command(repo.path(), &fetch_args, &[])
                        .with_context(|| format!("Failed to fetch target branch {}", target))?;
                    let output = std::process::Command::new("git")
                        .args(["rev-parse", "FETCH_HEAD"])
                        .current_dir(repo.path())
                        .output()
                        .with_context(|| {
                            "Failed to resolve FETCH_HEAD after target branch fetch"
                        })?;
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

    for pr in &prs {
        let (existing_change_id, _) =
            josh_core::trailers::parse_change_meta(&pr.head_commit_message);

        let head_oid = match git2::Oid::from_str(&pr.head_oid) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("PR #{}: bad head OID: {}", pr.number, e);
                report.skipped += 1;
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
                report.skipped += 1;
                continue;
            }
        };

        let base_oid = match git2::Oid::from_str(&pr.base_ref_oid) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("PR #{}: bad base OID: {}", pr.number, e);
                report.skipped += 1;
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
                report.skipped += 1;
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

        // Target branch for scoping: stacked changes encode the ultimate target
        // in the head ref (@changes/<target>/...); otherwise fall back to the
        // PR's immediate base.
        let target_branch = parse_changes_target(&pr.head_ref_name)
            .unwrap_or_else(|| pr.base_ref_name.trim_start_matches("refs/heads/"))
            .to_string();
        let remote_scope = josh_changes::ChangesRef::Remote {
            remote: remote_name.to_string(),
            branch: target_branch.clone(),
        };

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

                let merge_oid =
                    josh_changes::create_synthetic_merge_commit(repo, &pr_head, &target, &message)?;

                let merge = repo.find_commit(merge_oid)?;
                let mut change = josh_changes::Change::new(repo, &merge);
                change.set_base(target.id());
                change
            };

            josh_changes::store_diff_data(repo, &change, &remote_scope)?;
            Ok((change, pr.number))
        })();

        match result {
            Ok((change, pr_number)) => {
                match crate::sync_change_comments_by_pr_number(
                    &api,
                    &owner,
                    &repo_name,
                    repo,
                    &change,
                    pr_number,
                    &remote_scope,
                )
                .await
                {
                    Ok(n) => {
                        report.total_comments += n;
                        report.synced += 1;
                        println!("  PR #{}: synced {} comments", pr.number, n);
                    }
                    Err(e) => {
                        eprintln!("PR #{}: {} — skipping", pr.number, e);
                        report.skipped += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("PR #{}: {} — skipping", pr.number, e);
                report.skipped += 1;
            }
        }
    }

    println!(
        "Synced {} comments across {} PRs ({} skipped).",
        report.total_comments, report.synced, report.skipped
    );

    // Build the set of open change IDs from the PRs we just synced.
    let open_change_ids: std::collections::HashSet<String> = prs
        .iter()
        .map(|pr| {
            let (existing_id, _) = josh_core::trailers::parse_change_meta(&pr.head_commit_message);
            existing_id.unwrap_or_else(|| format!("{}/{}/pull/{}", owner, repo_name, pr.number))
        })
        .collect();

    // Iterate every (change, scope) pair under this remote -- changes may live
    // under multiple target-branch refs.
    let remote_scopes: Vec<josh_changes::ChangesRef> = josh_changes::all_changes_refs(repo)?
        .into_iter()
        .filter(|r| r.remote() == Some(remote_name))
        .collect();
    let mut all_changes: Vec<(josh_changes::Change, josh_changes::ChangesRef)> = Vec::new();
    for scope in &remote_scopes {
        for c in josh_changes::list_changes(repo, scope)? {
            all_changes.push((c, scope.clone()));
        }
    }

    for (change, remote_scope) in &all_changes {
        let change_id = match change.id() {
            Some(id) => id,
            None => continue,
        };

        if open_change_ids.contains(change_id) {
            continue;
        }

        // Determine the PR number for this change.
        let pr_number: i64 = match parse_pr_number_from_change_id(change_id, &owner, &repo_name) {
            Some(n) => n,
            None => {
                // Custom Change-Id; try reading stored PR data.
                match josh_changes::read_pr_data(repo, change_id, remote_scope) {
                    Ok(Some(json)) => match serde_json::from_str::<serde_json::Value>(&json) {
                        Ok(v) => match v.get("number").and_then(|n| n.as_i64()) {
                            Some(n) => n,
                            None => {
                                eprintln!(
                                    "  Change '{}': no PR number in stored data -- skipping",
                                    change_id
                                );
                                continue;
                            }
                        },
                        Err(e) => {
                            eprintln!(
                                "  Change '{}': invalid stored PR data: {} -- skipping",
                                change_id, e
                            );
                            continue;
                        }
                    },
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
            josh_changes::store_pr_data(repo, change_id, &json, remote_scope)?;
            eprintln!(
                "  Change '{}' (PR #{}): unexpectedly still OPEN on GitHub -- skipping deletion",
                change_id, pr_number
            );
            continue;
        }

        // Commit 1: store the updated PR data (final CLOSED/MERGED state).
        let json = serde_json::to_string(&pr_data)?;
        if let Err(e) = josh_changes::store_pr_data(repo, change_id, &json, remote_scope) {
            eprintln!(
                "  Change '{}' (PR #{}): failed to store updated PR data: {} -- skipping deletion",
                change_id, pr_number, e
            );
            continue;
        }

        // Commit 2: delete the change from the remote changes ref.
        if let Err(e) = josh_changes::delete_change(repo, change_id, remote_scope) {
            eprintln!(
                "  Change '{}' (PR #{}): failed to delete: {}",
                change_id, pr_number, e
            );
        } else {
            println!(
                "  Cleaned up '{}' (PR #{}: {})",
                change_id, pr_number, pr_data.state
            );
            report.cleaned += 1;
        }
    }

    if report.cleaned > 0 {
        println!("Cleaned up {} closed/merged changes.", report.cleaned);
    }

    if opts.push {
        for pr in &prs {
            let (existing_id, _) = josh_core::trailers::parse_change_meta(&pr.head_commit_message);
            let change_id = existing_id
                .unwrap_or_else(|| format!("{}/{}/pull/{}", owner, repo_name, pr.number));

            // Same target-branch derivation as the per-PR sync above.
            let target_branch = parse_changes_target(&pr.head_ref_name)
                .unwrap_or_else(|| pr.base_ref_name.trim_start_matches("refs/heads/"))
                .to_string();
            let remote_scope = josh_changes::ChangesRef::Remote {
                remote: remote_name.to_string(),
                branch: target_branch.clone(),
            };

            match api
                .find_pull_request_by_head(&owner, &repo_name, &pr.head_ref_name)
                .await
            {
                Ok(Some((pr_node_id, _, _))) => {
                    match crate::post_local_comments(
                        &api,
                        repo,
                        &change_id,
                        &pr_node_id,
                        &remote_scope,
                    )
                    .await
                    {
                        Ok(n) => {
                            report.total_posted += n;
                            if n > 0 {
                                println!("  PR #{}: posted {} local comments", pr.number, n);
                            }
                        }
                        Err(e) => {
                            eprintln!("  PR #{}: failed to post comments: {}", pr.number, e);
                        }
                    }

                    match crate::post_local_votes(
                        &api,
                        repo,
                        &change_id,
                        &pr_node_id,
                        &pr.head_oid,
                        &remote_scope,
                    )
                    .await
                    {
                        Ok(n) => {
                            report.total_votes_posted += n;
                            if n > 0 {
                                println!("  PR #{}: posted {} votes", pr.number, n);
                            }
                        }
                        Err(e) => {
                            eprintln!("  PR #{}: failed to post votes: {}", pr.number, e);
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
        println!(
            "Posted {} local comments and {} votes to GitHub.",
            report.total_posted, report.total_votes_posted
        );
    }

    Ok(report)
}

/// Extract the target branch name from a `@changes/<target>/<author>/<change-id>` ref name.
fn parse_changes_target(head_ref_name: &str) -> Option<&str> {
    let name = head_ref_name
        .strip_prefix("refs/heads/@changes/")
        .or_else(|| head_ref_name.strip_prefix("@changes/"))?;
    let mut end = 0;
    for part in name.split('/') {
        if part.contains('@') {
            return Some(name[..end].trim_end_matches('/'));
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
