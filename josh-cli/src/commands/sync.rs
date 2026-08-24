use anyhow::anyhow;

use crate::commands::scope::ScopeArgs;
use crate::forge::github;

/// Arguments for `josh changes sync`.
#[derive(Debug, clap::Parser)]
pub struct SyncArgs {
    #[command(flatten)]
    pub scope: ScopeArgs,

    /// Discard existing refs/josh/changes (for the resolved scope kind) before syncing.
    #[arg(long = "clean")]
    pub clean: bool,

    /// Push outbox comments and votes to GitHub (Remote scope only).
    #[arg(long = "push")]
    pub push: bool,

    /// Fetch all PR data fresh, ignoring the sync fingerprint cache.
    #[arg(long = "no-cache")]
    pub no_cache: bool,

    /// Seconds a sync fingerprint stays valid before forcing a refetch.
    #[arg(long = "cache-ttl", default_value_t = 3600)]
    pub cache_ttl: u64,
}

pub fn handle_sync(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_core::filter::check_experimental_features_enabled("josh changes sync")?;
    let head = transaction.head()?;
    let branch = head.short_branch().map(|s| s.to_string());

    let base_oid = if let Some(b) = &branch {
        match transaction.resolve_ref(&format!("refs/remotes/origin/{}", b))? {
            Some(oid) => josh_core::objects::peel_to_commit(transaction.odb(), oid)?,
            None => gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
        }
    } else {
        gix_hash::ObjectId::null(gix_hash::Kind::Sha1)
    };

    let resolved = args.scope.resolve(transaction)?;
    let remote_name = match &resolved {
        josh_changes::ChangesRef::Remote { remote, .. } => Some(remote.as_str()),
        josh_changes::ChangesRef::Local { .. } => None,
    };

    if args.clean {
        clean_scopes(transaction, remote_name)?;
    }

    match &resolved {
        josh_changes::ChangesRef::Remote { remote, .. } => sync_remote(args, transaction, remote),
        josh_changes::ChangesRef::Local { branch } => {
            sync_local(args, transaction, branch, head.commit, base_oid)
        }
    }
}

/// Delete every changes ref of the resolved kind. For Local: every
/// `refs/josh/changes/<branch>`. For Remote: every
/// `refs/josh/remotes/<remote>/changes/<branch>` for the chosen remote.
fn clean_scopes(
    transaction: &josh_core::cache::Transaction,
    remote_name: Option<&str>,
) -> anyhow::Result<()> {
    let mut to_delete: Vec<josh_changes::ChangesRef> = Vec::new();
    for scope in josh_changes::all_changes_refs(transaction)? {
        let keep = match (&scope, remote_name) {
            (josh_changes::ChangesRef::Local { .. }, None) => true,
            (josh_changes::ChangesRef::Remote { remote, .. }, Some(name)) => remote == name,
            _ => false,
        };
        if keep {
            to_delete.push(scope);
        }
    }
    for scope in to_delete {
        transaction.delete_ref(&scope.ref_name(), josh_core::cache::Expected::Any)?;
    }
    Ok(())
}

/// Sync the local changes ref for the current branch (no remote involved).
fn sync_local(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
    local_branch: &str,
    head_oid: gix_hash::ObjectId,
    base_oid: gix_hash::ObjectId,
) -> anyhow::Result<()> {
    if args.push {
        return Err(anyhow!(
            "--push requires --remote <name>; the Local ref has no posting target"
        ));
    }
    let changes = josh_changes::sync_changes(transaction, head_oid, base_oid, local_branch)?;
    if changes.is_empty() {
        println!("No local changes found.");
    }
    Ok(())
}

/// Sync against a GitHub remote: build the cache policy from args and
/// delegate to the GitHub change-management flow.
fn sync_remote(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
) -> anyhow::Result<()> {
    let policy = github::cache::CachePolicy::new(args.no_cache, args.cache_ttl);
    github::changes::sync(transaction, remote_name, &policy, args.push)
}
