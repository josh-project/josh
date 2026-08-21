//! GitHub change management: pull open PRs into local changes refs, sync
//! their comments, garbage-collect changes whose PRs closed, and push local
//! feedback back to GitHub.

use anyhow::{Context, anyhow};
use serde_json;

use josh_core::git::normalize_repo_path;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::pull_request::PrSummary;

use super::{api_connection_hint, make_api_connection};

use super::cache::CachePolicy;
use crate::config::read_remote_config;
use crate::forge::Forge;

/// Sync against GitHub: resolve the (owner, repo) pair for the remote and run
/// the async sync on a fresh tokio runtime.
pub fn sync(
    transaction: &josh_core::cache::Transaction,
    repo: &git2::Repository,
    remote_name: &str,
    policy: &CachePolicy,
    push: bool,
) -> anyhow::Result<()> {
    let (owner, repo_name) = resolve_github_remote(repo, Some(remote_name))?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_sync(
        transaction,
        repo,
        remote_name,
        &owner,
        &repo_name,
        policy,
        push,
    ))
}

/// Pull every open PR's changes and comments from GitHub, garbage-collect
/// changes whose PRs closed, and optionally push local feedback.
async fn run_sync(
    transaction: &josh_core::cache::Transaction,
    repo: &git2::Repository,
    remote_name: &str,
    owner: &str,
    repo_name: &str,
    policy: &CachePolicy,
    push: bool,
) -> anyhow::Result<()> {
    let api = make_api_connection()
        .await
        .with_context(api_connection_hint)?;

    let prs = api.list_open_pull_requests(owner, repo_name).await?;
    eprintln!("Found {} open PRs on GitHub.", prs.len());

    if prs.is_empty() {
        return Ok(());
    }

    let target_branch_shas = fetch_sync_objects(transaction, repo, owner, repo_name, &prs)?;

    let ctx = GithubSyncCtx {
        transaction,
        repo,
        api: &api,
        owner,
        repo_name,
        remote_name,
        target_branch_shas: &target_branch_shas,
    };

    // Only PRs whose fingerprint cache misses get their metadata fetched.
    let mut stats = SyncStats::default();
    let stale: Vec<&PrSummary> = prs
        .iter()
        .filter(|pr| {
            let change_id = change_id_for_pr(pr, owner, repo_name);
            let scope = remote_scope_for(remote_name, &target_branch_for_pr(pr));
            let hit = policy.lookup(transaction, &change_id, &scope, pr);
            if hit {
                stats.track_cached(pr.number);
            }
            !hit
        })
        .collect();

    sync_prs(&ctx, &stale, policy, &mut stats).await;
    stats.report();

    ctx.gc_closed_changes(&prs).await?;

    if push {
        ctx.push_local_feedback(&prs).await;
    }

    Ok(())
}

#[derive(Default)]
struct SyncStats {
    /// (PR number, comments synced) per fetched PR.
    synced: Vec<(i64, usize)>,
    /// (PR number, error) per PR that failed to sync.
    skipped: Vec<(i64, String)>,
    /// PR numbers served from the fingerprint cache.
    cached: Vec<i64>,
}

/// "0 PRs skipped" / "1 PR skipped: #8" / "4 PRs skipped: #8, #9 and 2 more PRs".
fn pr_count_list(prs: &[i64], noun: &str, suffix: &str) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    match prs.len() {
        0 => format!("0 {}{}{}", noun, plural(0), suffix),
        n => {
            let shown = prs
                .iter()
                .take(2)
                .map(|pr| format!("#{}", pr))
                .collect::<Vec<_>>()
                .join(", ");
            let more = n - n.min(2);
            if more == 0 {
                format!("{} {}{}{}: {}", n, noun, plural(n), suffix, shown)
            } else {
                format!(
                    "{} {}{}{}: {} and {} more {}{}",
                    n,
                    noun,
                    plural(n),
                    suffix,
                    shown,
                    more,
                    noun,
                    plural(more)
                )
            }
        }
    }
}

impl SyncStats {
    /// Record a PR whose metadata was fetched and stored.
    fn track_synced(&mut self, pr_number: i64, comments: usize) {
        self.synced.push((pr_number, comments));
    }

    /// Record a PR that failed to sync, with the error.
    fn track_skipped(&mut self, pr_number: i64, error: anyhow::Error) {
        self.skipped.push((pr_number, error.to_string()));
    }

    /// Record a PR served from the fingerprint cache.
    fn track_cached(&mut self, pr_number: i64) {
        self.cached.push(pr_number);
    }

    /// Print the end-of-sync summary.
    fn report(&self) {
        for (number, comments) in &self.synced {
            println!("  PR #{}: synced {} comments", number, comments);
        }
        for (number, error) in &self.skipped {
            eprintln!("  PR #{}: {} — skipped", number, error);
        }
        let total: usize = self.synced.iter().map(|(_, n)| n).sum();
        let skipped: Vec<i64> = self.skipped.iter().map(|(n, _)| *n).collect();
        println!(
            "Synced {} comments across {} PRs ({}, {}).",
            total,
            self.synced.len(),
            pr_count_list(&skipped, "PR", " skipped"),
            pr_count_list(&self.cached, "change", " cached"),
        );
    }
}

/// Fetch and store metadata (local change, diff data, comments) for each
/// given PR, accumulating counts into `stats`.
async fn sync_prs(
    ctx: &GithubSyncCtx<'_>,
    prs: &[&PrSummary],
    policy: &CachePolicy,
    stats: &mut SyncStats,
) {
    for pr in prs {
        let result = match ctx.fetch_pr_meta(pr).await {
            Ok(meta) => ctx.store_pr_meta(policy, pr, meta),
            Err(e) => Err(e),
        };
        match result {
            Ok(n) => stats.track_synced(pr.number, n),
            Err(e) => stats.track_skipped(pr.number, e),
        }
    }
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
    /// Fetch one PR's metadata from GitHub: resolve its change identity and
    /// base, then pull its comments. Performs no local writes; everything the
    /// store phase needs is returned in the `PrMeta`.
    async fn fetch_pr_meta(&self, pr: &PrSummary) -> anyhow::Result<PrMeta> {
        let (existing_change_id, _) =
            josh_core::trailers::parse_change_meta(&pr.head_commit_message);

        let head_oid =
            git2::Oid::from_str(&pr.head_oid).map_err(|e| anyhow!("bad head OID: {}", e))?;
        self.repo
            .find_commit(head_oid)
            .map_err(|_| anyhow!("head commit {} not available from GitHub", pr.head_oid))?;

        let base_oid =
            git2::Oid::from_str(&pr.base_ref_oid).map_err(|e| anyhow!("bad base OID: {}", e))?;
        self.repo
            .find_commit(base_oid)
            .map_err(|_| anyhow!("base commit {} not available from GitHub", pr.base_ref_oid))?;

        // For stacked changes the base is the merge-base against the ultimate
        // target branch's tip; otherwise the merge-base against the PR's
        // immediate base. Only needed when the head carries a change-id;
        // synthetic merges take the immediate base directly.
        let change_base = if existing_change_id.is_some() {
            let against = parse_changes_target(&pr.head_ref_name)
                .and_then(|t| self.target_branch_shas.get(&t))
                .copied()
                .unwrap_or(base_oid);

            Some(josh_core::objects::merge_base(
                &self.transaction.odb()?,
                against,
                head_oid,
            )?)
        } else {
            None
        };

        let pr_data = self
            .api
            .get_pr_comments(self.owner, self.repo_name, pr.number)
            .await?;

        Ok(PrMeta {
            change_id: existing_change_id
                .unwrap_or_else(|| change_id_for_pr(pr, self.owner, self.repo_name)),
            remote_scope: remote_scope_for(self.remote_name, &target_branch_for_pr(pr)),
            head: head_oid,
            target: base_oid,
            change_base,
            pr_data,
        })
    }

    /// Store one PR's fetched metadata locally: build its change (a synthetic
    /// merge commit unless the head already carries a change-id), store diff
    /// data and comments, and record the sync fingerprint. Contains every
    /// write of the per-PR sync. Returns the number of comments synced.
    fn store_pr_meta(
        &self,
        policy: &CachePolicy,
        pr: &PrSummary,
        meta: PrMeta,
    ) -> anyhow::Result<usize> {
        let change = if let Some(base) = meta.change_base {
            println!(
                "PR #{}: head commit has change-id '{}'",
                pr.number, meta.change_id
            );
            let mut change = josh_changes::Change::new(self.transaction, meta.head)?;
            change.set_base(base);
            change
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
            let mut message = pr.title.clone();
            if !pr.body.is_empty() {
                message.push_str("\n\n");
                message.push_str(&pr.body);
            }
            message.push_str(&format!("\n\nChange-Id: {}\n", meta.change_id));

            let merge_oid = josh_changes::create_synthetic_merge_commit(
                self.transaction,
                meta.head,
                meta.target,
                &message,
            )?;
            let mut change = josh_changes::Change::new(self.transaction, merge_oid)?;
            change.set_base(meta.target);
            change
        };

        josh_changes::store_diff_data(self.transaction, &change, &meta.remote_scope)?;

        let json = serde_json::to_string(&meta.pr_data)?;
        josh_changes::store_pr_data(self.transaction, &meta.change_id, &json, &meta.remote_scope)?;

        let fetched = josh_github_changes::fetched_comments(&meta.pr_data);
        let written = josh_changes::store_fetched_comments(
            self.transaction,
            &change,
            &fetched,
            &meta.remote_scope,
        )?;
        josh_github_changes::record_fetched_comments(
            self.transaction,
            &meta.change_id,
            &written,
            &meta.remote_scope,
        )?;

        policy.record(self.transaction, &meta.change_id, &meta.remote_scope, pr);
        Ok(written.len())
    }
}

/// One PR's metadata as fetched from GitHub, before any local writes.
struct PrMeta {
    /// Trailer change-id if the head commit has one, otherwise the synthetic
    /// `{owner}/{repo}/pull/{N}` id.
    change_id: String,
    remote_scope: josh_changes::ChangesRef,
    head: git2::Oid,
    /// Immediate base commit of the PR.
    target: git2::Oid,
    /// Resolved change base for change-id'd heads; `None` for synthetic merges.
    change_base: Option<git2::Oid>,
    pr_data: josh_github_graphql::operations::pull_request::PrData,
}

impl GithubSyncCtx<'_> {
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

    /// Post local comments and votes that haven't been pushed to GitHub yet.
    async fn push_local_feedback(&self, prs: &[PrSummary]) {
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
                    total_posted += self
                        .push_pr_comments(pr, &pr_node_id, &change_id, &remote_scope)
                        .await;
                    total_votes_posted += self
                        .push_pr_votes(pr, &pr_node_id, &change_id, &remote_scope)
                        .await;
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
    }

    /// Post pending local comments for one PR and record their GitHub IDs.
    /// Returns the number posted and recorded; a load failure aborts the
    /// comment push for this PR and returns 0.
    async fn push_pr_comments(
        &self,
        pr: &PrSummary,
        pr_node_id: &str,
        change_id: &str,
        remote_scope: &josh_changes::ChangesRef,
    ) -> usize {
        let comments = match josh_changes::read_comments(self.transaction, change_id, remote_scope)
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  PR #{}: failed to load local comments: {}", pr.number, e);
                return 0;
            }
        };
        let pending = match josh_github_changes::pending_comments(
            self.transaction,
            change_id,
            remote_scope,
            comments,
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("  PR #{}: failed to load local comments: {}", pr.number, e);
                return 0;
            }
        };
        let outcome = josh_github_changes::post_comments(self.api, pr_node_id, &pending).await;
        let mut recorded = 0usize;
        for p in &outcome.posted {
            if let Err(e) = josh_github_changes::store_github_id(
                self.transaction,
                change_id,
                &p.local_id,
                &p.github_id,
                remote_scope,
            ) {
                eprintln!(
                    "  PR #{}: failed to record posted comment: {}",
                    pr.number, e
                );
                continue;
            }
            recorded += 1;
        }
        if recorded > 0 {
            println!("  PR #{}: posted {} local comments", pr.number, recorded);
        }
        if let Some(e) = outcome.error {
            eprintln!("  PR #{}: failed to post comments: {}", pr.number, e);
        }
        recorded
    }

    /// Post pending local votes for one PR and record their GitHub IDs.
    /// Returns the number posted and recorded; a load failure aborts the
    /// vote push for this PR and returns 0.
    async fn push_pr_votes(
        &self,
        pr: &PrSummary,
        pr_node_id: &str,
        change_id: &str,
        remote_scope: &josh_changes::ChangesRef,
    ) -> usize {
        let outbox_votes =
            match josh_changes::list_outbox_votes(self.transaction, change_id, remote_scope) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("  PR #{}: failed to load local votes: {}", pr.number, e);
                    return 0;
                }
            };
        let pending_votes = match josh_github_changes::pending_votes(
            self.transaction,
            change_id,
            remote_scope,
            &outbox_votes,
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  PR #{}: failed to load local votes: {}", pr.number, e);
                return 0;
            }
        };
        let outcome =
            josh_github_changes::post_votes(self.api, pr_node_id, &pr.head_oid, &pending_votes)
                .await;
        let mut recorded_votes = 0usize;
        for (user, data) in &outcome.posted {
            if let Err(e) = josh_github_changes::store_github_vote_id(
                self.transaction,
                change_id,
                user,
                data,
                remote_scope,
            ) {
                eprintln!("  PR #{}: failed to record posted vote: {}", pr.number, e);
                continue;
            }
            recorded_votes += 1;
        }
        // Drop outbox entries whose post is now reflected in gh_vote_ids.
        // Safe to call unconditionally -- it's a no-op when nothing needs cleaning.
        if let Err(e) = josh_github_changes::cleanup_posted_outbox_votes(
            self.transaction,
            change_id,
            remote_scope,
            &outbox_votes,
        ) {
            eprintln!(
                "  PR #{}: failed to clean up posted votes: {}",
                pr.number, e
            );
        }
        if recorded_votes > 0 {
            println!("  PR #{}: posted {} votes", pr.number, recorded_votes);
        }
        if let Some(e) = outcome.error {
            eprintln!("  PR #{}: failed to post votes: {}", pr.number, e);
        }
        recorded_votes
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

/// Read the remote config and return the GitHub (owner, repo) pair.
fn resolve_github_remote(
    repo: &git2::Repository,
    remote: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let remote_name = remote.unwrap_or("origin");
    let repo_path = normalize_repo_path(repo.path());
    let remote_config = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

    if remote_config.forge != Some(Forge::Github) {
        return Err(anyhow!("sync is only supported for GitHub remotes"));
    }

    josh_github_changes::repo::parse_owner_repo(&remote_config.url)
}

fn fetch_sync_objects(
    transaction: &josh_core::cache::Transaction,
    repo: &git2::Repository,
    owner: &str,
    repo_name: &str,
    prs: &[PrSummary],
) -> anyhow::Result<std::collections::HashMap<String, git2::Oid>> {
    const SCRATCH: &str = "refs/josh/sync-tips";

    let github_url = format!("https://github.com/{}/{}", owner, repo_name);
    let mut refspecs: Vec<String> = Vec::new();
    let mut targets: Vec<String> = Vec::new();
    let mut seen_oids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut seen_targets: std::collections::HashSet<String> = std::collections::HashSet::new();

    for pr in prs {
        for oid in [&pr.head_oid, &pr.base_ref_oid] {
            if seen_oids.insert(oid.as_str()) {
                refspecs.push(oid.clone());
            }
        }
        if let Some(target) = parse_changes_target(&pr.head_ref_name)
            && seen_targets.insert(target.clone())
        {
            refspecs.push(format!("+refs/heads/{0}:{SCRATCH}/{0}", target));
            targets.push(target);
        }
    }

    let mut fetch_args: Vec<&str> = Vec::with_capacity(3 + refspecs.len());
    fetch_args.push("fetch");
    fetch_args.push(&github_url);
    fetch_args.push("--no-tags");
    fetch_args.extend(refspecs.iter().map(String::as_str));
    transaction
        .spawn_git(&fetch_args, &[])
        .with_context(|| "Failed to fetch objects from GitHub")?;

    // Read the captured tips back out of the scratch refs, then remove them.
    let mut tips = std::collections::HashMap::new();
    for target in targets {
        let ref_name = format!("{SCRATCH}/{target}");
        let Some(oid) = transaction.resolve_ref(&ref_name)? else {
            return Err(anyhow!("target branch {} missing after fetch", target));
        };
        transaction.delete_ref(&ref_name, josh_core::cache::Expected::Any)?;
        tips.insert(target, oid);
    }

    // Refresh ODB so git2 sees the newly fetched objects.
    repo.odb()?.refresh()?;
    Ok(tips)
}
