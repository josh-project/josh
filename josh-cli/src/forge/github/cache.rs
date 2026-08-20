//! Sync fingerprint cache policy for the GitHub change sync: when a
//! stored fingerprint still matches a PR's list-query fields, the full
//! comment fetch is skipped.

use josh_github_graphql::operations::pull_request::PrSummary;

/// Sync fingerprint cache policy: whether reads are forced to miss,
/// how long a fingerprint stays fresh, and the timestamp (computed once per
/// sync) that freshness checks and new fingerprints are measured against.
pub struct CachePolicy {
    no_cache: bool,
    ttl: u64,
    now: u64,
}

impl CachePolicy {
    pub fn new(no_cache: bool, ttl: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        CachePolicy { no_cache, ttl, now }
    }

    /// true = cache hit (skip). Read errors are logged inside and treated as miss.
    pub(super) fn lookup(
        &self,
        transaction: &josh_core::cache::Transaction,
        change_id: &str,
        remote_scope: &josh_changes::ChangesRef,
        pr: &PrSummary,
    ) -> bool {
        if self.no_cache {
            return false;
        }
        match josh_github_changes::read_sync_fingerprint(transaction, change_id, remote_scope) {
            Ok(Some(fp)) if fp.matches(pr) && fp.is_fresh(self.now, self.ttl) => true,
            Ok(_) => false,
            Err(e) => {
                eprintln!(
                    "  PR #{}: failed to read sync cache: {} — fetching fresh",
                    pr.number, e
                );
                false
            }
        }
    }

    /// Persist fingerprint after a successful fresh fetch; logs and swallows errors.
    pub(super) fn record(
        &self,
        transaction: &josh_core::cache::Transaction,
        change_id: &str,
        remote_scope: &josh_changes::ChangesRef,
        pr: &PrSummary,
    ) {
        // The fetch was fresh, so keep the cache coherent even
        // under --no-cache (which only forces reads to miss).
        let fp = josh_github_changes::SyncFingerprint::from_summary(pr, self.now);
        if let Err(e) =
            josh_github_changes::store_sync_fingerprint(transaction, change_id, &fp, remote_scope)
        {
            eprintln!("  PR #{}: failed to write sync cache: {}", pr.number, e);
        }
    }
}
