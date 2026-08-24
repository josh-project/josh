//! GitHub change management: pull open PRs into local changes refs, sync
//! their comments, garbage-collect changes whose PRs closed, and push local
//! feedback back to GitHub.

use anyhow::{Context, anyhow};
use std::str::FromStr;

use josh_core::git::normalize_repo_path;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::pull_request::{PrData, PrSummary};

use super::{api_connection_hint, make_api_connection};

use super::cache::CachePolicy;
use crate::config::read_remote_config;
use crate::forge::Forge;

/// Sync against GitHub: resolve the (owner, repo) pair for the remote and run
/// the async sync on a fresh tokio runtime.
pub fn sync(
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
    policy: &CachePolicy,
    push: bool,
) -> anyhow::Result<()> {
    let (owner, repo_name) = resolve_github_remote(transaction, Some(remote_name))?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_sync(
        transaction,
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

    let target_branch_shas = fetch_sync_objects(transaction, owner, repo_name, &prs)?;

    let ctx = GithubSyncCtx {
        transaction,
        api: &api,
        owner,
        repo_name,
        remote_name,
        target_branch_shas: &target_branch_shas,
    };

    let mut stats = SyncStats::default();

    // Only PRs whose fingerprint cache misses get their metadata fetched.
    {
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

        for pr in stale {
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

    {
        let gc_candidates = ctx.gc_candidates(&prs)?;
        let states = ctx.fetch_gc_states(gc_candidates).await;
        ctx.apply_gc(states, &mut stats)?;
    }

    if push {
        for pending in ctx.prepare_feedback(&prs) {
            stats.track_pushed(&ctx.publish_feedback(pending).await);
        }
    }

    stats.report();
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
    /// (change id, PR number, final state) per cleaned change.
    cleaned: Vec<(String, i64, String)>,
    /// (change id, PR number, reason) per change that could not be cleaned.
    gc_skipped: Vec<(String, i64, String)>,
    /// (PR number, posted comments, posted votes) per pushed PR.
    pushed: Vec<(i64, usize, usize)>,
    /// (PR number, reason) per failed or partially failed feedback push.
    push_failures: Vec<(i64, String)>,
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

    /// Record a change deleted because its PR closed or merged.
    fn track_cleaned(&mut self, change_id: &str, pr_number: i64, state: &str) {
        self.cleaned
            .push((change_id.to_string(), pr_number, state.to_string()));
    }

    /// Record one PR's feedback publish outcome.
    fn track_pushed(&mut self, outcome: &PublishOutcome) {
        self.pushed.push((
            outcome.pr_number,
            outcome.posted_comments,
            outcome.posted_votes,
        ));
        for e in &outcome.errors {
            self.push_failures.push((outcome.pr_number, e.clone()));
        }
    }

    /// Record a change whose cleanup was skipped, with the reason.
    fn track_gc_skipped(&mut self, change_id: &str, pr_number: i64, reason: String) {
        self.gc_skipped
            .push((change_id.to_string(), pr_number, reason));
    }

    /// Print the end-of-sync summary.
    fn report(&self) {
        for (number, comments) in &self.synced {
            println!("  PR #{}: synced {} comments", number, comments);
        }
        for (number, error) in &self.skipped {
            eprintln!("  PR #{}: {} — skipped", number, error);
        }
        for (change_id, pr_number, state) in &self.cleaned {
            println!(
                "  Cleaned up '{}' (PR #{}: {})",
                change_id, pr_number, state
            );
        }
        for (change_id, pr_number, reason) in &self.gc_skipped {
            eprintln!(
                "  Change '{}' (PR #{}): {} -- skipping",
                change_id, pr_number, reason
            );
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
        if !self.cleaned.is_empty() {
            println!("Cleaned up {} closed/merged changes.", self.cleaned.len());
        }
        if !self.pushed.is_empty() || !self.push_failures.is_empty() {
            for (number, error) in &self.push_failures {
                eprintln!("  PR #{}: {}", number, error);
            }
            for (number, comments, votes) in &self.pushed {
                if *comments > 0 {
                    println!("  PR #{}: posted {} local comments", number, comments);
                }
                if *votes > 0 {
                    println!("  PR #{}: posted {} votes", number, votes);
                }
            }
            let total_comments: usize = self.pushed.iter().map(|(_, c, _)| c).sum();
            let total_votes: usize = self.pushed.iter().map(|(_, _, v)| v).sum();
            println!(
                "Posted {} local comments and {} votes to GitHub.",
                total_comments, total_votes
            );
        }
    }
}

/// Shared context for one sync run: repo and transaction handles, the GitHub
/// API connection, the identity of the remote being synced, and the fetched
/// target-branch tips. Per-phase behavior is documented on the methods.
struct GithubSyncCtx<'a> {
    transaction: &'a josh_core::cache::Transaction,
    api: &'a GithubApiConnection,
    owner: &'a str,
    repo_name: &'a str,
    remote_name: &'a str,
    target_branch_shas: &'a std::collections::HashMap<String, gix_hash::ObjectId>,
}

impl GithubSyncCtx<'_> {
    /// Fetch one PR's metadata from GitHub: resolve its change identity and
    /// base, then pull its comments. Performs no local writes; everything the
    /// store phase needs is returned in the `PrMeta`.
    async fn fetch_pr_meta(&self, pr: &PrSummary) -> anyhow::Result<PrMeta> {
        let (existing_change_id, _) =
            josh_core::trailers::parse_change_meta(&pr.head_commit_message);

        let head_oid = gix_hash::ObjectId::from_str(&pr.head_oid)
            .map_err(|e| anyhow!("bad head OID: {}", e))?;
        let odb = self.transaction.odb();
        josh_core::objects::CommitData::read(odb, head_oid)
            .map_err(|_| anyhow!("head commit {} not available from GitHub", pr.head_oid))?;

        let base_oid = gix_hash::ObjectId::from_str(&pr.base_ref_oid)
            .map_err(|e| anyhow!("bad base OID: {}", e))?;
        josh_core::objects::CommitData::read(odb, base_oid)
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
                self.transaction.odb(),
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

        josh_github_changes::store_pr_data(
            self.transaction,
            &meta.change_id,
            &meta.pr_data,
            &meta.remote_scope,
        )?;

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
    head: gix_hash::ObjectId,
    /// Immediate base commit of the PR.
    target: gix_hash::ObjectId,
    /// Resolved change base for change-id'd heads; `None` for synthetic merges.
    change_base: Option<gix_hash::ObjectId>,
    pr_data: josh_github_graphql::operations::pull_request::PrData,
}

/// A local change whose PR is absent from the open list, awaiting a closure
/// check against GitHub before deletion.
struct GcCandidate {
    change_id: String,
    remote_scope: josh_changes::ChangesRef,
    pr_number: i64,
}

/// Pending local feedback for one PR, loaded from local refs and ready to
/// publish. Comment and vote loads fail independently.
struct PendingFeedback {
    pr_number: i64,
    head_ref_name: String,
    head_oid: String,
    change_id: String,
    remote_scope: josh_changes::ChangesRef,
    comments: anyhow::Result<josh_changes::PendingComments>,
    votes: anyhow::Result<PendingVotes>,
}

/// Pending votes plus the full outbox, needed for post-publish cleanup.
struct PendingVotes {
    pending: Vec<(String, josh_changes::VoteData)>,
    outbox: Vec<(String, josh_changes::VoteData)>,
}

/// Result of publishing one PR's feedback: what was posted and recorded,
/// plus message-ready descriptions of every failure along the way.
struct PublishOutcome {
    pr_number: i64,
    posted_comments: usize,
    posted_votes: usize,
    errors: Vec<String>,
}

impl GithubSyncCtx<'_> {
    /// Collect local changes under this remote whose PRs are not in the open
    /// list and which map to a PR number. Local reads only -- no network, no
    /// writes.
    fn gc_candidates(&self, prs: &[PrSummary]) -> anyhow::Result<Vec<GcCandidate>> {
        let open_change_ids: std::collections::HashSet<String> = prs
            .iter()
            .map(|pr| change_id_for_pr(pr, self.owner, self.repo_name))
            .collect();

        // Iterate every (change, scope) pair under this remote -- changes may live
        // under multiple target-branch refs.
        let remote_scopes: Vec<josh_changes::ChangesRef> =
            josh_changes::all_changes_refs(self.transaction)?
                .into_iter()
                .filter(|r| r.remote() == Some(self.remote_name))
                .collect();

        let mut candidates = Vec::new();
        for scope in &remote_scopes {
            for change in josh_changes::list_changes(self.transaction, scope)? {
                let Some(change_id) = change.id() else {
                    continue;
                };
                if open_change_ids.contains(change_id) {
                    continue;
                }
                if let Some(pr_number) = resolve_pr_number(
                    self.transaction,
                    self.owner,
                    self.repo_name,
                    change_id,
                    scope,
                ) {
                    candidates.push(GcCandidate {
                        change_id: change_id.to_string(),
                        remote_scope: scope.clone(),
                        pr_number,
                    });
                }
            }
        }
        Ok(candidates)
    }

    /// Fetch the current PR state for each candidate. Network only -- no
    /// local writes; per-PR fetch failures are carried into the action stage.
    async fn fetch_gc_states(
        &self,
        candidates: Vec<GcCandidate>,
    ) -> Vec<(GcCandidate, anyhow::Result<PrData>)> {
        let mut states = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let state = self
                .api
                .get_pr_comments(self.owner, self.repo_name, candidate.pr_number)
                .await;
            states.push((candidate, state));
        }
        states
    }

    /// Apply garbage collection: record the final PR state and delete the
    /// change for every candidate whose PR is closed/merged. All local writes
    /// happen here; outcomes are tracked in `stats`.
    fn apply_gc(
        &self,
        states: Vec<(GcCandidate, anyhow::Result<PrData>)>,
        stats: &mut SyncStats,
    ) -> anyhow::Result<()> {
        for (candidate, state) in states {
            let GcCandidate {
                change_id,
                remote_scope,
                pr_number,
            } = &candidate;
            let pr_number = *pr_number;

            let pr_data = match state {
                Ok(d) => d,
                Err(e) => {
                    stats.track_gc_skipped(
                        change_id,
                        pr_number,
                        format!("failed to fetch PR data: {}", e),
                    );
                    continue;
                }
            };

            // Record the final PR state for every candidate, open or closed;
            // a failed ref write skips the candidate like any other failure.
            if let Err(e) = josh_github_changes::store_pr_data(
                self.transaction,
                change_id,
                &pr_data,
                remote_scope,
            ) {
                stats.track_gc_skipped(
                    change_id,
                    pr_number,
                    format!("failed to store updated PR data: {}", e),
                );
                continue;
            }

            // Guard: if the PR is still open, do not delete the change. The
            // state string is the Debug form of the GraphQL enum ("Open").
            if pr_data.state.eq_ignore_ascii_case("open") {
                stats.track_gc_skipped(
                    change_id,
                    pr_number,
                    "unexpectedly still OPEN on GitHub".to_string(),
                );
                continue;
            }

            // Delete the change from the remote changes ref.
            match josh_changes::delete_change(
                self.transaction,
                change_id,
                remote_scope,
                &[
                    josh_github_changes::GITHUB_PR_DATA_PATH,
                    josh_github_changes::GITHUB_COMMENT_NODE_IDS_PATH,
                    josh_github_changes::GITHUB_VOTE_NODE_IDS_PATH,
                    josh_github_changes::GITHUB_CACHE_PATH,
                ],
            ) {
                Ok(()) => stats.track_cleaned(change_id, pr_number, &pr_data.state),
                Err(e) => {
                    stats.track_gc_skipped(change_id, pr_number, format!("failed to delete: {}", e))
                }
            }
        }

        Ok(())
    }

    /// Load pending local feedback (comments and votes) for every PR. Local
    /// reads only -- no network, no writes. Comment and vote loads fail
    /// independently; failures are carried into the publish stage.
    fn prepare_feedback(&self, prs: &[PrSummary]) -> Vec<PendingFeedback> {
        prs.iter()
            .map(|pr| {
                let change_id = change_id_for_pr(pr, self.owner, self.repo_name);
                let remote_scope = remote_scope_for(self.remote_name, &target_branch_for_pr(pr));
                let comments =
                    josh_changes::read_comments(self.transaction, &change_id, &remote_scope)
                        .and_then(|c| {
                            josh_github_changes::pending_comments(
                                self.transaction,
                                &change_id,
                                &remote_scope,
                                c,
                            )
                        });
                let votes =
                    josh_changes::list_outbox_votes(self.transaction, &change_id, &remote_scope)
                        .and_then(|outbox| {
                            josh_github_changes::pending_votes(
                                self.transaction,
                                &change_id,
                                &remote_scope,
                                &outbox,
                            )
                            .map(|pending| PendingVotes { pending, outbox })
                        });
                PendingFeedback {
                    pr_number: pr.number,
                    head_ref_name: pr.head_ref_name.clone(),
                    head_oid: pr.head_oid.clone(),
                    change_id,
                    remote_scope,
                    comments,
                    votes,
                }
            })
            .collect()
    }

    /// Publish one PR's pending feedback to GitHub and record the resulting
    /// GitHub IDs locally. All network mutations and local writes of the push
    /// phase happen here; failures are collected as messages in the outcome.
    async fn publish_feedback(&self, pending: PendingFeedback) -> PublishOutcome {
        let mut outcome = PublishOutcome {
            pr_number: pending.pr_number,
            posted_comments: 0,
            posted_votes: 0,
            errors: Vec::new(),
        };

        let comments = match &pending.comments {
            Ok(c) => Some(c),
            Err(e) => {
                outcome
                    .errors
                    .push(format!("failed to load local comments: {}", e));
                None
            }
        };
        let votes = match &pending.votes {
            Ok(v) => Some(v),
            Err(e) => {
                outcome
                    .errors
                    .push(format!("failed to load local votes: {}", e));
                None
            }
        };
        if comments.is_none() && votes.is_none() {
            return outcome;
        }

        let pr_node_id = match self
            .api
            .find_pull_request_by_head(self.owner, self.repo_name, &pending.head_ref_name, None)
            .await
        {
            Ok(Some((id, _, _))) => id,
            Ok(None) => {
                outcome.errors.push(format!(
                    "no open PR found for {} -- skipping feedback push",
                    pending.head_ref_name
                ));
                return outcome;
            }
            Err(e) => {
                outcome.errors.push(format!(
                    "failed to look up PR for {}: {}",
                    pending.head_ref_name, e
                ));
                return outcome;
            }
        };

        if let Some(c) = comments {
            let post = josh_github_changes::post_comments(self.api, &pr_node_id, c).await;
            let written: Vec<(String, String)> = post
                .posted
                .iter()
                .map(|p| (p.local_id.clone(), p.github_id.clone()))
                .collect();
            match josh_github_changes::store_comment_node_ids(
                self.transaction,
                &pending.change_id,
                &written,
                &pending.remote_scope,
            ) {
                Ok(()) => outcome.posted_comments += post.posted.len(),
                Err(e) => outcome
                    .errors
                    .push(format!("failed to record posted comments: {}", e)),
            }
            if let Some(e) = post.error {
                outcome
                    .errors
                    .push(format!("failed to post comments: {}", e));
            }
        }

        if let Some(v) = votes {
            let post = josh_github_changes::post_votes(
                self.api,
                &pr_node_id,
                &pending.head_oid,
                &v.pending,
            )
            .await;
            match josh_github_changes::store_vote_node_ids(
                self.transaction,
                &pending.change_id,
                &post.posted,
                &pending.remote_scope,
            ) {
                Ok(()) => outcome.posted_votes += post.posted.len(),
                Err(e) => outcome
                    .errors
                    .push(format!("failed to record posted votes: {}", e)),
            }
            // Drop outbox entries whose post is now reflected in gh_vote_node_ids.
            // Safe unconditionally -- no-op when nothing needs cleaning.
            if let Err(e) = josh_github_changes::cleanup_posted_outbox_votes(
                self.transaction,
                &pending.change_id,
                &pending.remote_scope,
                &v.outbox,
            ) {
                outcome
                    .errors
                    .push(format!("failed to clean up posted votes: {}", e));
            }
            if let Some(e) = post.error {
                outcome.errors.push(format!("failed to post votes: {}", e));
            }
        }

        outcome
    }
}

/// Determine the PR number for a change: parse it from a synthetic
/// `{owner}/{repo}/pull/{N}` change ID, or fall back to stored PR data for
/// custom Change-Ids. Returns None for purely local changes, which GC never
/// touches.
fn resolve_pr_number(
    transaction: &josh_core::cache::Transaction,
    owner: &str,
    repo: &str,
    change_id: &str,
    remote_scope: &josh_changes::ChangesRef,
) -> Option<i64> {
    if let Some(n) = change_id
        .strip_prefix(&format!("{}/{}/pull/", owner, repo))
        .and_then(|n| n.parse().ok())
    {
        return Some(n);
    }

    // Custom Change-Id; read the PR number from stored PR data. A corrupt or
    // schema-drifted blob is reported instead of silently dropping the change
    // from GC.
    match josh_github_changes::read_pr_data(transaction, change_id, remote_scope) {
        Ok(Some(data)) => Some(data.number),
        Ok(None) => None,
        Err(e) => {
            eprintln!(
                "Change '{}': stored PR data failed to load, excluding from GC: {}",
                change_id, e
            );
            None
        }
    }
}

/// Extract the target branch name from a stacked-changes ref name.
fn parse_changes_target(head_ref_name: &str) -> Option<String> {
    match josh_changes::StackedRef::parse(head_ref_name)? {
        josh_changes::StackedRef::ChangeRef(change) => Some(change.target().to_string()),
        josh_changes::StackedRef::StackHead { target, .. } => Some(target),
    }
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

/// Read the remote config and return the GitHub (owner, repo) pair.
fn resolve_github_remote(
    transaction: &josh_core::cache::Transaction,
    remote: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let remote_name = remote.unwrap_or("origin");
    let repo_path = normalize_repo_path(transaction.path());
    let remote_config = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

    if remote_config.forge != Some(Forge::Github) {
        return Err(anyhow!("sync is only supported for GitHub remotes"));
    }

    josh_github_changes::repo::parse_owner_repo(&remote_config.url)
}

fn fetch_sync_objects(
    transaction: &josh_core::cache::Transaction,
    owner: &str,
    repo_name: &str,
    prs: &[PrSummary],
) -> anyhow::Result<std::collections::HashMap<String, gix_hash::ObjectId>> {
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

    Ok(tips)
}
