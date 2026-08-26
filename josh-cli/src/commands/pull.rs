use anyhow::Context;

use josh_changes::{StackedChangeRef, StackedRef};
use josh_core::trailers::commit_change_meta;

use crate::commands::fetch::{self, FetchArgs};
use crate::porcelain::RefUpdate;

#[derive(Debug, clap::Parser)]
pub struct PullArgs {
    /// Remote name (or URL) to pull from
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// Ref to pull (branch, tag, or commit-ish)
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,
}

/// The result of integrating fetched refs into the current branch.
#[derive(Debug)]
pub enum IntegrateReport {
    /// Nothing to do (upstream unchanged or already contained in local HEAD).
    UpToDate,
    /// Local branch was fast-forwarded to the upstream tip.
    FastForward { branch: String },
    /// All local change commits were already applied upstream; the branch was
    /// rewound to the upstream tip, dropping them.
    Rewound {
        branch: String,
        skipped: Vec<String>,
    },
    /// Some local changes were already applied upstream (dropped), the rest
    /// were re-applied (cherry-picked) on top of the upstream tip.
    Restacked {
        branch: String,
        skipped: Vec<String>,
        kept: usize,
    },
}

fn short_oid(oid: gix_hash::ObjectId) -> String {
    oid.to_string()[..7].to_string()
}

/// Compute git's stable patch-id for a commit's diff against its first parent.
/// Used as an opportunistic content hash: a change that was merely restacked
/// (rebased without content edits) keeps its patch-id even though the commit
/// and tree oids all change.
fn patch_id(
    transaction: &josh_core::cache::Transaction,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<String> {
    let output = transaction
        .git_command(
            &[
                "diff-tree",
                "-p",
                "--root",
                "--no-commit-id",
                &oid.to_string(),
            ],
            &[],
        )?
        .with_stdout(std::process::Stdio::piped())
        .spawn()?;
    let diff = String::from_utf8(output.stdout).context("invalid git diff-tree output")?;

    let mut child = std::process::Command::new("git")
        .args(["patch-id", "--stable"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn git patch-id")?;

    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(diff.as_bytes())?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!("git patch-id failed for commit {}", oid);
    }

    let text = String::from_utf8(output.stdout).context("invalid git patch-id output")?;
    Ok(text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string())
}

/// Decide whether a change ref update represents a content change, comparing
/// patch-ids of the old and new split commits. Failures fail open (counted as
/// changed). Without a transaction (tests), every update counts as changed.
fn change_content_changed(
    transaction: Option<&josh_core::cache::Transaction>,
    old: gix_hash::ObjectId,
    new: gix_hash::ObjectId,
) -> bool {
    let Some(transaction) = transaction else {
        return true;
    };

    match (patch_id(transaction, old), patch_id(transaction, new)) {
        (Ok(old_id), Ok(new_id)) => old_id != new_id,
        _ => true,
    }
}

/// Render one summary line per interesting ref update, reframing the
/// stacked-changes refs (`@changes/`) as a change count and hiding the
/// internal `@base/`/`@heads/` refs entirely.
///
/// A `@changes/` update only counts towards the change count when the change's
/// content actually changed (stable patch-id differs), so republishing a
/// restacked but otherwise untouched stack does not over-report.
pub fn render_fetch_summary(
    updates: &[RefUpdate],
    remote: &str,
    transaction: Option<&josh_core::cache::Transaction>,
) -> anyhow::Result<Vec<String>> {
    let prefix = format!("refs/remotes/{}/", remote);
    let mut lines = Vec::new();
    let mut changes = 0usize;

    for update in updates {
        if let RefUpdate::Rejected { reference, reason } = update {
            anyhow::bail!(
                "git fetch rejected update to {}{}",
                reference,
                reason
                    .as_deref()
                    .map(|r| format!(" ({})", r))
                    .unwrap_or_default()
            );
        }

        let Some(name) = update.reference().strip_prefix(&prefix) else {
            continue;
        };

        match StackedRef::parse(name) {
            Some(StackedRef::ChangeRef(StackedChangeRef::Change { .. })) => {
                let changed = match update {
                    RefUpdate::New { .. } => true,
                    RefUpdate::FastForward { old, new, .. }
                    | RefUpdate::Forced { old, new, .. } => {
                        change_content_changed(transaction, *old, *new)
                    }
                    _ => false,
                };
                if changed {
                    changes += 1;
                }
                continue;
            }
            // @base and @heads refs are internal plumbing; hide them entirely.
            Some(_) => continue,
            None => {}
        }

        let line = match update {
            RefUpdate::New { .. } => format!("new branch {}", name),
            RefUpdate::Deleted { .. } => format!("deleted branch {}", name),
            RefUpdate::Forced { old, new, .. } => format!(
                "force-updated {} ({}..{})",
                name,
                short_oid(*old),
                short_oid(*new)
            ),
            RefUpdate::FastForward { old, new, .. } => format!(
                "updated {} ({}..{})",
                name,
                short_oid(*old),
                short_oid(*new)
            ),
            RefUpdate::Rejected { .. } => unreachable!("rejected updates bail out above"),
        };
        lines.push(line);
    }

    if changes > 0 {
        lines.push(format!(
            "updated {} change{}",
            changes,
            if changes == 1 { "" } else { "s" }
        ));
    }

    Ok(lines)
}

/// Resolve the remote-tracking ref the current branch should integrate.
///
/// The configured upstream must belong to the remote being pulled; pulling a
/// different remote than the one the branch tracks is an error (mirroring
/// `git pull <other-remote>` without a branch argument).
fn resolve_upstream_ref(
    transaction: &josh_core::cache::Transaction,
    branch_ref: &str,
    remote: &str,
) -> anyhow::Result<String> {
    let expected_prefix = format!("refs/remotes/{}/", remote);

    if let Some(upstream) = transaction.upstream_ref(branch_ref)? {
        {
            let upstream = upstream.as_str();
            if !upstream.starts_with(&expected_prefix) {
                return Err(anyhow::anyhow!(
                    "the current branch tracks '{}', which does not belong to remote '{}'",
                    upstream,
                    remote
                ));
            }
            return Ok(upstream.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "no upstream configured for the current branch on remote '{}'",
        remote
    ))
}

/// Collect Change-Ids of all commits reachable from `tip` but not from `base`.
/// Walks all parents (not just first-parent) so changes landed via merge
/// commits (e.g. by a merge queue) are detected as well.
fn upstream_change_ids(
    transaction: &josh_core::cache::Transaction,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let odb = transaction.odb();
    let commits = if base.is_null() {
        let mut walk = josh_core::objects::RevWalk::new(odb);
        walk.push(tip)?;
        walk.into_topo_vec(|_| false)?
    } else {
        josh_core::objects::RangeWalk::new(odb, |oid| {
            josh_core::cache::compute_sequence_number(transaction, oid)
        })
        .into_topo_vec(tip, base)?
    };

    let mut ids = std::collections::HashSet::new();
    for oid in commits {
        let commit = josh_core::objects::CommitData::read(odb, oid)?;
        if let (Some(id), _) = commit_change_meta(&commit) {
            ids.insert(id);
        }
    }

    Ok(ids)
}

/// Cherry-pick `commits` (oldest first) on top of `tip`, in memory.
/// Bails out on conflicts without writing any refs.
fn restack_commits(
    transaction: &josh_core::cache::Transaction,
    tip: gix_hash::ObjectId,
    commits: &[gix_hash::ObjectId],
) -> anyhow::Result<(gix_hash::ObjectId, usize)> {
    use josh_core::objects::CommitData;

    let odb = transaction.odb();
    let mut acc = CommitData::read(odb, tip)?;
    let mut kept = 0usize;

    for &oid in commits {
        let commit = CommitData::read(odb, oid)?;
        let parent = commit
            .first_parent_id()
            .with_context(|| format!("cannot restack root commit {}", short_oid(oid)))?;
        let parent_tree = CommitData::read(odb, parent)?.tree_id()?;

        let tree_oid = josh_core::objects::merge_trees(
            odb,
            parent_tree,
            acc.tree_id()?,
            commit.tree_id()?,
        )
        .with_context(|| {
            format!(
                "conflict while restacking change commit {} onto upstream; resolve manually",
                short_oid(oid)
            )
        })?;
        if tree_oid == acc.tree_id()? {
            // The change became empty on top of the new base (e.g. its content
            // already landed upstream without a matching Change-Id).
            continue;
        }

        let new_oid = josh_core::objects::write_commit_with_signatures_of(
            odb,
            &commit,
            tree_oid,
            &[acc.id()],
            std::str::from_utf8(commit.message_raw()?).unwrap_or_default(),
        )?;
        acc = CommitData::read(odb, new_oid)?;
        kept += 1;
    }

    Ok((acc.id(), kept))
}

/// Integrate the fetched state of `remote` into the current branch.
///
/// Linear-history only: fast-forward, rewind over already-applied changes
/// (detected by Change-Id trailers), and cherry-pick the remaining local
/// changes on top. Local commits without a Change-Id cannot be matched against
/// the upstream stack, so they are always kept and restacked. Anything more
/// complex (merge commits, conflicts, dirty worktree) bails out with refs
/// untouched.
pub fn integrate(
    transaction: &josh_core::cache::Transaction,
    remote: &str,
) -> anyhow::Result<IntegrateReport> {
    let head = transaction.head().context("Failed to resolve HEAD")?;
    let branch_ref = head
        .branch()
        .ok_or_else(|| anyhow::anyhow!("HEAD is detached; cannot integrate pull"))?
        .to_string();
    let branch = branch_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&branch_ref)
        .to_string();

    let upstream_refname = resolve_upstream_ref(transaction, &branch_ref, remote)?;
    let new = transaction
        .resolve_ref(&upstream_refname)?
        .ok_or_else(|| anyhow::anyhow!("failed to resolve upstream ref '{}'", upstream_refname))?;
    let new = josh_core::objects::peel_to_commit(transaction.odb(), new)
        .with_context(|| format!("failed to resolve upstream ref '{}'", upstream_refname))?;
    let old = head.commit;
    // The ref's stored target, which the CAS guard on the write below compares against;
    // differs from the peeled `old` only if the branch points at an annotated tag.
    let old_target = head.target;

    if old == new {
        return Ok(IntegrateReport::UpToDate);
    }

    let merge_base = josh_core::objects::merge_base(transaction.odb(), old, new)?;

    // Upstream is already contained in the local branch: nothing to integrate.
    if merge_base == new {
        return Ok(IntegrateReport::UpToDate);
    }

    let (new_tip, report) = if merge_base == old {
        (new, IntegrateReport::FastForward { branch })
    } else {
        let odb = transaction.odb();
        let mut walk = josh_core::objects::RevWalk::new(odb);
        walk.simplify_first_parent();
        walk.push(old)?;
        let mut local_commits = walk.into_topo_vec(|oid| oid == merge_base)?;
        local_commits.reverse();

        for &oid in &local_commits {
            let commit = josh_core::objects::CommitData::read(odb, oid)?;
            if commit.parent_count() > 1 {
                anyhow::bail!(
                    "local branch '{}' contains merge commits; cannot integrate automatically",
                    branch
                );
            }
        }

        let applied = upstream_change_ids(transaction, new, merge_base)?;

        let mut skipped = Vec::new();
        let mut remaining = Vec::new();
        for &oid in &local_commits {
            let commit = josh_core::objects::CommitData::read(transaction.odb(), oid)?;
            match commit_change_meta(&commit) {
                (Some(id), _) if applied.contains(&id) => skipped.push(id),
                // Keep unmatched commits; restacking drops content that already landed.
                _ => remaining.push(oid),
            }
        }

        if remaining.is_empty() {
            (new, IntegrateReport::Rewound { branch, skipped })
        } else {
            let (tip, kept) = restack_commits(transaction, new, &remaining)?;
            (
                tip,
                IntegrateReport::Restacked {
                    branch,
                    skipped,
                    kept,
                },
            )
        }
    };

    // The checkout below reads the restacked commits through the repository handle, which only
    // sees what is on disk.
    transaction.flush_mem_odb()?;

    // Update the worktree first (safe: refuses to clobber conflicting local
    // modifications), only then move the branch ref.
    josh_core::git::checkout_commit(transaction, new_tip)
        .context("failed to update working tree; local changes conflict with the pulled state")?;
    transaction.update_ref(
        &branch_ref,
        josh_core::cache::Expected::At(old_target),
        new_tip,
        "josh changes pull",
    )?;

    Ok(report)
}

/// Stash uncommitted changes to tracked files so the worktree update in
/// `integrate` cannot conflict with them. Returns true if a stash was created.
fn autostash_push(transaction: &josh_core::cache::Transaction) -> anyhow::Result<bool> {
    if !josh_core::git::has_tracked_changes(transaction)? {
        return Ok(false);
    }

    transaction
        .spawn_git(
            &["stash", "push", "--quiet", "-m", "josh changes pull"],
            &[],
        )
        .context("failed to stash local changes")?;
    eprintln!("Stashed uncommitted changes");
    Ok(true)
}

/// Re-apply a stash created by `autostash_push`. On conflict the stash entry is
/// kept (git semantics) and the user is pointed at it.
fn autostash_pop(transaction: &josh_core::cache::Transaction) {
    if let Err(e) = transaction.spawn_git(&["stash", "pop", "--quiet"], &[]) {
        eprintln!(
            "Warning: could not re-apply stashed changes ({}); they remain in the stash",
            e
        );
    } else {
        eprintln!("Re-applied stashed changes");
    }
}

/// Handle the `josh changes pull` command: fetch (with projection filtering),
/// print a curated summary, then integrate into the current branch.
///
/// Integration is always rebase-style; uncommitted changes are stashed before
/// integrating and re-applied afterwards (autostash).
pub fn handle_pull(
    args: &PullArgs,
    transaction: &josh_core::cache::Transaction,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    let fetch_args = FetchArgs {
        remote: args.remote.clone(),
        rref: args.rref.clone(),
    };

    let updates = fetch::handle_fetch(&fetch_args, transaction, distributed_cache)?;
    for line in render_fetch_summary(&updates, &args.remote, Some(transaction))? {
        eprintln!("{}", line);
    }

    let stashed = autostash_push(transaction)?;
    let result = integrate(transaction, &args.remote);
    if stashed {
        // Re-apply even when integrating failed, restoring the pre-pull state.
        autostash_pop(transaction);
    }

    match result? {
        IntegrateReport::UpToDate => {
            eprintln!("Already up to date.");
        }
        IntegrateReport::FastForward { branch } => {
            eprintln!("Fast-forwarded {}", branch);
        }
        IntegrateReport::Rewound { branch, skipped } => {
            eprintln!(
                "Skipping {} already applied change{}: {}",
                skipped.len(),
                if skipped.len() == 1 { "" } else { "s" },
                skipped.join(", ")
            );
            eprintln!("Rewound {}", branch);
        }
        IntegrateReport::Restacked {
            branch,
            skipped,
            kept,
        } => {
            if !skipped.is_empty() {
                eprintln!(
                    "Skipping {} already applied change{}: {}",
                    skipped.len(),
                    if skipped.len() == 1 { "" } else { "s" },
                    skipped.join(", ")
                );
            }
            eprintln!(
                "Restacked {}: kept {} change{}",
                branch,
                kept,
                if kept == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const OID_A: &str = "af180e6da554e60815593af48d419ac0e719c47a";
    const OID_B: &str = "2bf1cefd82c96e5d7478ff834c59194d40e539c4";

    fn oid(s: &str) -> gix_hash::ObjectId {
        gix_hash::ObjectId::from_str(s).unwrap()
    }

    fn fast_forward(old: &str, new: &str, reference: &str) -> RefUpdate {
        RefUpdate::FastForward {
            old: oid(old),
            new: oid(new),
            reference: reference.to_string(),
        }
    }

    fn new_ref(new: &str, reference: &str) -> RefUpdate {
        RefUpdate::New {
            new: oid(new),
            reference: reference.to_string(),
        }
    }

    #[test]
    fn upstream_change_ids_walks_every_parent_between_base_and_tip() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = josh_core::cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let signature = gix_actor::Signature {
            name: "Test".into(),
            email: "test@example.com".into(),
            time: gix_actor::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        let commit = |parents: &[gix_hash::ObjectId], change: Option<&str>| {
            let message = change
                .map(|id| format!("Subject\n\nChange: {id}\n"))
                .unwrap_or_else(|| "Root".to_string());
            josh_core::objects::write_commit(
                transaction.odb(),
                gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1),
                parents,
                &signature,
                &signature,
                &message,
            )
            .unwrap()
        };

        let root = commit(&[], None);
        let left = commit(&[root], Some("left"));
        let right = commit(&[root], Some("right"));
        let merge = commit(&[left, right], Some("merge"));

        assert_eq!(
            upstream_change_ids(&transaction, merge, root).unwrap(),
            ["left", "right", "merge"]
                .map(String::from)
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn summary_renders_branch_updates() {
        let updates = vec![
            fast_forward(OID_A, OID_B, "refs/remotes/origin/master"),
            new_ref(OID_A, "refs/remotes/origin/feature"),
        ];

        let lines = render_fetch_summary(&updates, "origin", None).unwrap();

        assert_eq!(
            lines,
            vec!["updated master (af180e6..2bf1cef)", "new branch feature",]
        );
    }

    #[test]
    fn summary_counts_changes_and_hides_internal_refs() {
        let updates = vec![
            new_ref(OID_A, "refs/remotes/origin/@changes/master/a@b.c/1234"),
            new_ref(OID_A, "refs/remotes/origin/@changes/master/a@b.c/5678"),
            new_ref(OID_A, "refs/remotes/origin/@base/master/a@b.c/1234"),
            fast_forward(OID_A, OID_B, "refs/remotes/origin/@heads/master/a@b.c"),
            fast_forward(OID_A, OID_B, "refs/remotes/origin/master"),
        ];

        let lines = render_fetch_summary(&updates, "origin", None).unwrap();

        assert_eq!(
            lines,
            vec!["updated master (af180e6..2bf1cef)", "updated 2 changes"]
        );
    }

    #[test]
    fn summary_ignores_refs_from_other_remotes() {
        let updates = vec![fast_forward(OID_A, OID_B, "refs/remotes/remote2/master")];
        assert_eq!(
            render_fetch_summary(&updates, "origin", None).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn summary_rejected_update_errors() {
        let updates = vec![RefUpdate::Rejected {
            reference: "refs/remotes/origin/master".to_string(),
            reason: Some("non-fast-forward".to_string()),
        }];
        assert!(render_fetch_summary(&updates, "origin", None).is_err());
    }

    #[test]
    fn summary_forced_change_counts_without_transaction() {
        let updates = vec![RefUpdate::Forced {
            old: oid(OID_A),
            new: oid(OID_B),
            reference: "refs/remotes/origin/@changes/master/a@b.c/1234".to_string(),
        }];

        let lines = render_fetch_summary(&updates, "origin", None).unwrap();

        assert_eq!(lines, vec!["updated 1 change"]);
    }

    #[test]
    fn summary_deleted_change_is_not_an_update() {
        let updates = vec![RefUpdate::Deleted {
            old: oid(OID_A),
            reference: "refs/remotes/origin/@changes/master/a@b.c/1234".to_string(),
        }];

        let lines = render_fetch_summary(&updates, "origin", None).unwrap();

        assert_eq!(lines, Vec::<String>::new());
    }
}
