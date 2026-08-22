use anyhow::{anyhow, Context};

use josh_core::git::normalize_repo_path;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::pull_request::PrSummary;

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
                    josh_changes::all_changes_refs(transaction)?
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
    let _ = branch;

    if opts.clean {
        // Delete every changes ref under this remote.
        let mut to_delete: Vec<josh_changes::ChangesRef> = Vec::new();
        for scope in josh_changes::all_changes_refs(transaction)? {
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

    fetch_pr_objects(transaction, repo, &owner, &repo_name, &prs)?;
    let target_branch_shas = fetch_target_branch_tips(transaction, repo, &owner, &repo_name, &prs)?;

    let ctx = GithubSyncCtx {
        transaction,
        repo,
        api: &api,
        owner: &owner,
        repo_name: &repo_name,
        remote_name,
        target_branch_shas: &target_branch_shas,
    };

    for pr in &prs {
        match ctx.sync_single_pr(pr).await {
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

    println!(
        "Synced {} comments across {} PRs ({} skipped).",
        report.total_comments, report.synced, report.skipped
    );

    report.cleaned = ctx.gc_closed_changes(&prs).await?;

    if opts.push {
        let (posted, votes) = ctx.push_local_feedback(&prs).await;
        report.total_posted = posted;
        report.total_votes_posted = votes;
    }

    Ok(report)
}

/// Context for GitHub-backed sync phases.
struct GithubSyncCtx<'a> {
    transaction: &'a josh_core::cache::Transaction,
    repo: &'a git2::Repository,
    api: &'a GithubApiConnection,
    owner: &'a str,
    repo_name: &'a str,
    remote_name: &'a str,
    target_branch_shas: &'a std::collections::HashMap<String, git2::Oid>,
}

impl GithubSyncCtx<'_> {
    /// Sync a single PR: build its local change, store diff data, and pull its
    /// comments from GitHub. Returns the number of comments synced.
    async fn sync_single_pr(&self, pr: &PrSummary) -> anyhow::Result<usize> {
        let (existing_change_id, _) =
            josh_core::trailers::parse_change_meta(&pr.head_commit_message);

        let head_oid =
            git2::Oid::from_str(&pr.head_oid).map_err(|e| anyhow!("bad head OID: {}", e))?;
        let pr_head = self
            .repo
            .find_commit(head_oid)
            .map_err(|_| anyhow!("head commit {} not available from GitHub", pr.head_oid))?;

        let base_oid =
            git2::Oid::from_str(&pr.base_ref_oid).map_err(|e| anyhow!("bad base OID: {}", e))?;
        let target = self
            .repo
            .find_commit(base_oid)
            .map_err(|_| anyhow!("base commit {} not available from GitHub", pr.base_ref_oid))?;

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

        let target_branch = target_branch_for_pr(pr);
        let remote_scope = remote_scope_for(self.remote_name, &target_branch);

        let change = self.build_change_for_pr(
            pr,
            existing_change_id.is_some(),
            &pr_head,
            &target,
            &remote_scope,
        )?;

        let Some(change_id) = change.id() else {
            return Ok(0);
        };

        let pr_data = self
            .api
            .get_pr_comments(self.owner, self.repo_name, pr.number)
            .await?;
        let json = serde_json::to_string(&pr_data)?;
        josh_changes::store_pr_data(self.transaction, change_id, &json, &remote_scope)?;

        let fetched = crate::fetched_comments(&pr_data);
        let written = josh_changes::store_fetched_comments(
            self.transaction,
            &change,
            &fetched,
            &remote_scope,
        )?;
        crate::record_fetched_comments(self.transaction, change_id, &written, &remote_scope)?;
        Ok(written.len())
    }

    /// Delete local changes whose PRs are no longer open on GitHub, after
    /// recording their final PR state. Returns the number of cleaned changes.
    async fn gc_closed_changes(&self, prs: &[PrSummary]) -> anyhow::Result<usize> {
        let open_change_ids = collect_open_change_ids(prs, self.owner, self.repo_name);

        // Iterate every (change, scope) pair under this remote -- changes may live
        // under multiple target-branch refs.
        let remote_scopes: Vec<josh_changes::ChangesRef> =
            josh_changes::all_changes_refs(self.transaction)?
                .into_iter()
                .filter(|r| r.remote() == Some(self.remote_name))
                .collect();
        let mut all_changes: Vec<(josh_changes::Change, josh_changes::ChangesRef)> = Vec::new();
        for scope in &remote_scopes {
            for c in josh_changes::list_changes(self.transaction, scope)? {
                all_changes.push((c, scope.clone()));
            }
        }
        let mut cleaned = 0usize;

        for (change, remote_scope) in &all_changes {
            let Some(change_id) = change.id() else {
                continue;
            };

            if open_change_ids.contains(change_id) {
                continue;
            }

            let Some(pr_number) = self.resolve_pr_number(change_id, remote_scope) else {
                continue;
            };

            // Fetch the current PR data from GitHub.
            let pr_data = match self
                .api
                .get_pr_comments(self.owner, self.repo_name, pr_number)
                .await
            {
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
                josh_changes::store_pr_data(self.transaction, change_id, &json, remote_scope)?;
                eprintln!(
                    "  Change '{}' (PR #{}): unexpectedly still OPEN on GitHub \
                     -- skipping deletion",
                    change_id, pr_number
                );
                continue;
            }

            // Commit 1: store the updated PR data (final CLOSED/MERGED state).
            let json = serde_json::to_string(&pr_data)?;
            if let Err(e) =
                josh_changes::store_pr_data(self.transaction, change_id, &json, remote_scope)
            {
                eprintln!(
                    "  Change '{}' (PR #{}): failed to store updated PR data: {} \
                     -- skipping deletion",
                    change_id, pr_number, e
                );
                continue;
            }

            // Commit 2: delete the change from the remote changes ref.
            if let Err(e) = josh_changes::delete_change(self.transaction, change_id, remote_scope) {
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
        Ok(cleaned)
    }

    /// Determine the PR number for a change: parse it from a synthetic
    /// `{owner}/{repo}/pull/{N}` change ID, or fall back to stored PR data for
    /// custom Change-Ids. Returns None for purely local changes.
    fn resolve_pr_number(
        &self,
        change_id: &str,
        remote_scope: &josh_changes::ChangesRef,
    ) -> Option<i64> {
        if let Some(n) = parse_pr_number_from_change_id(change_id, self.owner, self.repo_name) {
            return Some(n);
        }

        // Custom Change-Id; try reading stored PR data.
        match josh_changes::read_pr_data(self.transaction, change_id, remote_scope) {
            Ok(Some(json)) => match serde_json::from_str::<serde_json::Value>(&json) {
                Ok(v) => match v.get("number").and_then(|n| n.as_i64()) {
                    Some(n) => Some(n),
                    None => {
                        eprintln!(
                            "  Change '{}': no PR number in stored data -- skipping",
                            change_id
                        );
                        None
                    }
                },
                Err(e) => {
                    eprintln!(
                        "  Change '{}': invalid stored PR data: {} -- skipping",
                        change_id, e
                    );
                    None
                }
            },
            Ok(None) => {
                // Purely local change with no PR data at all.
                None
            }
            Err(e) => {
                eprintln!(
                    "  Change '{}': failed to read PR data: {} -- skipping",
                    change_id, e
                );
                None
            }
        }
    }

    /// Build the local change for a PR: use the head commit directly when it
    /// carries a change-id trailer, otherwise create a synthetic merge commit
    /// with one. Stores the change's diff data under the given scope.
    fn build_change_for_pr(
        &self,
        pr: &PrSummary,
        has_change_id: bool,
        pr_head: &git2::Commit,
        target: &git2::Commit,
        remote_scope: &josh_changes::ChangesRef,
    ) -> anyhow::Result<josh_changes::Change> {
        let change = if has_change_id {
            let mut change = josh_changes::Change::new(self.transaction, pr_head.id())?;
            let base = match parse_changes_target(&pr.head_ref_name)
                .and_then(|t| self.target_branch_shas.get(&t))
            {
                Some(tip) => self.repo.merge_base(*tip, pr_head.id())?,
                None => self.repo.merge_base(target.id(), pr_head.id())?,
            };
            change.set_base(base);
            change
        } else {
            let change_id = change_id_for_pr(pr, self.owner, self.repo_name);
            let mut message = pr.title.clone();
            if !pr.body.is_empty() {
                message.push_str("\n\n");
                message.push_str(&pr.body);
            }
            message.push_str(&format!("\n\nChange-Id: {}\n", change_id));

            let merge_oid = josh_changes::create_synthetic_merge_commit(
                self.transaction,
                pr_head.id(),
                target.id(),
                &message,
            )?;

            let mut change = josh_changes::Change::new(self.transaction, merge_oid)?;
            change.set_base(target.id());
            change
        };

        josh_changes::store_diff_data(self.transaction, &change, remote_scope)?;
        Ok(change)
    }

    /// Post local comments and votes that haven't been pushed to GitHub yet.
    /// Returns the number of comments and votes posted.
    async fn push_local_feedback(&self, prs: &[PrSummary]) -> (usize, usize) {
        let mut total_posted = 0usize;
        let mut total_votes_posted = 0usize;
        for pr in prs {
            let change_id = change_id_for_pr(pr, self.owner, self.repo_name);
            let target_branch = target_branch_for_pr(pr);
            let remote_scope = remote_scope_for(self.remote_name, &target_branch);

            match self
                .api
                .find_pull_request_by_head(self.owner, self.repo_name, &pr.head_ref_name, None)
                .await
            {
                Ok(Some((pr_node_id, _, _))) => {
                    'comments: {
                        let comments = match josh_changes::read_comments(
                            self.transaction,
                            &change_id,
                            &remote_scope,
                        ) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!(
                                    "  PR #{}: failed to load local comments: {}",
                                    pr.number, e
                                );
                                break 'comments;
                            }
                        };
                        let pending = match crate::pending_comments(
                            self.transaction,
                            &change_id,
                            &remote_scope,
                            comments,
                        ) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!(
                                    "  PR #{}: failed to load local comments: {}",
                                    pr.number, e
                                );
                                break 'comments;
                            }
                        };
                        let outcome = crate::post_comments(self.api, &pr_node_id, &pending).await;
                        let mut recorded = 0usize;
                        for p in &outcome.posted {
                            if let Err(e) = crate::store_github_id(
                                self.transaction,
                                &change_id,
                                &p.local_id,
                                &p.github_id,
                                &remote_scope,
                            ) {
                                eprintln!(
                                    "  PR #{}: failed to record posted comment: {}",
                                    pr.number, e
                                );
                                continue;
                            }
                            recorded += 1;
                        }
                        total_posted += recorded;
                        if recorded > 0 {
                            println!("  PR #{}: posted {} local comments", pr.number, recorded);
                        }
                        if let Some(e) = outcome.error {
                            eprintln!("  PR #{}: failed to post comments: {}", pr.number, e);
                        }
                    }

                    'votes: {
                        let outbox_votes = match josh_changes::list_outbox_votes(
                            self.transaction,
                            &change_id,
                            &remote_scope,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("  PR #{}: failed to load local votes: {}", pr.number, e);
                                break 'votes;
                            }
                        };
                        let pending_votes = match crate::pending_votes(
                            self.transaction,
                            &change_id,
                            &remote_scope,
                            &outbox_votes,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("  PR #{}: failed to load local votes: {}", pr.number, e);
                                break 'votes;
                            }
                        };
                        let outcome =
                            crate::post_votes(self.api, &pr_node_id, &pr.head_oid, &pending_votes)
                                .await;
                        let mut recorded_votes = 0usize;
                        for (user, data) in &outcome.posted {
                            if let Err(e) = crate::store_github_vote_id(
                                self.transaction,
                                &change_id,
                                user,
                                data,
                                &remote_scope,
                            ) {
                                eprintln!(
                                    "  PR #{}: failed to record posted vote: {}",
                                    pr.number, e
                                );
                                continue;
                            }
                            recorded_votes += 1;
                        }
                        // Drop outbox entries whose post is now reflected in gh_vote_ids.
                        // Safe to call unconditionally -- it's a no-op when nothing needs cleaning.
                        if let Err(e) = crate::cleanup_posted_outbox_votes(
                            self.transaction,
                            &change_id,
                            &remote_scope,
                            &outbox_votes,
                        ) {
                            eprintln!(
                                "  PR #{}: failed to clean up posted votes: {}",
                                pr.number, e
                            );
                        }
                        total_votes_posted += recorded_votes;
                        if recorded_votes > 0 {
                            println!("  PR #{}: posted {} votes", pr.number, recorded_votes);
                        }
                        if let Some(e) = outcome.error {
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
            total_posted, total_votes_posted
        );
        (total_posted, total_votes_posted)
    }
}

/// Extract the target branch name from a stacked-changes ref name.
fn parse_changes_target(head_ref_name: &str) -> Option<String> {
    match josh_changes::StackedRef::parse(head_ref_name)? {
        josh_changes::StackedRef::ChangeRef(change) => Some(change.target().to_string()),
        josh_changes::StackedRef::StackHead { target, .. } => Some(target),
    }
}

/// Extract the PR number from a synthetic change ID of the form `{owner}/{repo}/pull/{N}`.
fn parse_pr_number_from_change_id(change_id: &str, owner: &str, repo: &str) -> Option<i64> {
    let prefix = format!("{}/{}/pull/", owner, repo);
    change_id.strip_prefix(&prefix)?.parse().ok()
}

/// Derive the change ID for a PR: the change-id from the head commit's trailers
/// if present, otherwise a synthetic `{owner}/{repo}/pull/{N}` ID.
fn change_id_for_pr(pr: &PrSummary, owner: &str, repo: &str) -> String {
    let (existing_id, _) = josh_core::trailers::parse_change_meta(&pr.head_commit_message);
    existing_id.unwrap_or_else(|| format!("{}/{}/pull/{}", owner, repo, pr.number))
}

/// Determine the target branch for scoping a PR's changes: stacked changes encode
/// the ultimate target in the head ref (@changes/<target>/...); otherwise fall
/// back to the PR's immediate base.
fn target_branch_for_pr(pr: &PrSummary) -> String {
    parse_changes_target(&pr.head_ref_name).unwrap_or_else(|| {
        pr.base_ref_name
            .trim_start_matches("refs/heads/")
            .to_string()
    })
}

/// Build the remote changes-ref scope for a target branch.
fn remote_scope_for(remote_name: &str, target_branch: &str) -> josh_changes::ChangesRef {
    josh_changes::ChangesRef::Remote {
        remote: remote_name.to_string(),
        branch: target_branch.to_string(),
    }
}

/// Build the set of change IDs for the given open PRs.
fn collect_open_change_ids(
    prs: &[PrSummary],
    owner: &str,
    repo: &str,
) -> std::collections::HashSet<String> {
    prs.iter()
        .map(|pr| change_id_for_pr(pr, owner, repo))
        .collect()
}

/// Fetch the head and base commits of all given PRs by SHA from GitHub.
fn fetch_pr_objects(
    transaction: &josh_core::cache::Transaction,
    repo: &git2::Repository,
    owner: &str,
    repo_name: &str,
    prs: &[PrSummary],
) -> anyhow::Result<()> {
    // Collect all unique OIDs: PR head commits + target branch tips.
    let mut oids: Vec<&str> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for pr in prs {
        if seen.insert(pr.head_oid.as_str()) {
            oids.push(&pr.head_oid);
        }
        if seen.insert(pr.base_ref_oid.as_str()) {
            oids.push(&pr.base_ref_oid);
        }
    }

    if oids.is_empty() {
        return Ok(());
    }

    let github_url = format!("https://github.com/{}/{}", owner, repo_name);
    let mut fetch_args: Vec<&str> = Vec::with_capacity(3 + oids.len());
    fetch_args.push("fetch");
    fetch_args.push(&github_url);
    fetch_args.push("--no-tags");
    fetch_args.extend(oids);
    transaction
        .spawn_git(&fetch_args, &[])
        .with_context(|| "Failed to fetch objects from GitHub")?;

    // Refresh ODB so git2 sees the newly fetched objects.
    repo.odb()?.refresh()?;
    Ok(())
}

/// Fetch the tips of the target branches referenced by @changes/... head refs.
/// Returns a map from target branch name to its fetched tip commit.
fn fetch_target_branch_tips(
    transaction: &josh_core::cache::Transaction,
    repo: &git2::Repository,
    owner: &str,
    repo_name: &str,
    prs: &[PrSummary],
) -> anyhow::Result<std::collections::HashMap<String, git2::Oid>> {
    let github_url = format!("https://github.com/{}/{}", owner, repo_name);
    let mut tips = std::collections::HashMap::new();
    let mut seen_targets = std::collections::HashSet::new();
    for pr in prs {
        let Some(target) = parse_changes_target(&pr.head_ref_name) else {
            continue;
        };
        if !seen_targets.insert(target.clone()) {
            continue;
        }
        let refspec = format!("refs/heads/{}", target);
        transaction
            .spawn_git(&["fetch", &github_url, "--no-tags", &refspec], &[])
            .with_context(|| format!("Failed to fetch target branch {}", target))?;
        let oid = repo
            .find_reference("FETCH_HEAD")
            .context("Failed to find FETCH_HEAD after target branch fetch")?
            .peel_to_commit()
            .context("Failed to peel FETCH_HEAD to commit")?
            .id();
        tips.insert(target, oid);
    }
    if !tips.is_empty() {
        repo.odb()?.refresh()?;
    }
    Ok(tips)
}
