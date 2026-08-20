//! Sync fingerprint cache on changes refs: snapshot of the list-query fields
//! from the last full comment fetch, so sync can skip unchanged PRs.
//! Persisted as a JSON blob at `gh_cache/<change-id>/fingerprint` —
//! the path is part of the on-disk format and must not change.

use josh_changes::{encode_change_id_path, ChangesRef};
use josh_core::cache::Transaction;
use josh_github_graphql::operations::pull_request::PrSummary;

use serde::{Deserialize, Serialize};

const LEAF: &str = "fingerprint";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncFingerprint {
    pub updated_at: String,
    pub head_oid: String,
    pub comment_count: i64,
    pub review_count: i64,
    /// Unix seconds of the last full `get_pr_comments` fetch.
    pub fetched_at: u64,
}

impl SyncFingerprint {
    pub fn from_summary(pr: &PrSummary, fetched_at: u64) -> Self {
        Self {
            updated_at: pr.updated_at.clone(),
            head_oid: pr.head_oid.clone(),
            comment_count: pr.comment_count,
            review_count: pr.review_count,
            fetched_at,
        }
    }

    /// True when the stored snapshot matches the fresh list-query fields
    /// (ignores `fetched_at`; freshness is `is_fresh`).
    pub fn matches(&self, pr: &PrSummary) -> bool {
        self.updated_at == pr.updated_at
            && self.head_oid == pr.head_oid
            && self.comment_count == pr.comment_count
            && self.review_count == pr.review_count
    }

    /// True when the snapshot is at most `ttl_secs` old at `now`
    /// (saturating: a future `fetched_at` from clock skew counts as fresh).
    pub fn is_fresh(&self, now: u64, ttl_secs: u64) -> bool {
        now.saturating_sub(self.fetched_at) <= ttl_secs
    }
}

/// Read the stored sync fingerprint for a change, if present and well-formed.
/// A missing or corrupt blob is a cache miss, not an error.
pub fn read_sync_fingerprint(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Option<SyncFingerprint>> {
    let path = std::path::Path::new("gh_cache").join(encode_change_id_path(change_id));
    let map = josh_changes::read_blob_map(transaction, scope, &path)?;
    let Some(json) = map.get(LEAF) else {
        return Ok(None);
    };
    // Corrupt blob → cache miss (safe refetch), not an error.
    Ok(serde_json::from_str(json).ok())
}

/// Store the sync fingerprint for a change after a successful full fetch.
pub fn store_sync_fingerprint(
    transaction: &Transaction,
    change_id: &str,
    fingerprint: &SyncFingerprint,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(fingerprint)?;
    let blob_oid = transaction.repo().blob(json.as_bytes())?;
    let path = std::path::Path::new("gh_cache")
        .join(encode_change_id_path(change_id))
        .join(LEAF);
    josh_changes::write_changes_tree(transaction, &path, blob_oid, None, None, scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> PrSummary {
        PrSummary {
            number: 7,
            title: "title".to_string(),
            body: "body".to_string(),
            base_ref_name: "master".to_string(),
            base_ref_oid: "0".repeat(40),
            head_ref_name: "feature".to_string(),
            head_oid: "1".repeat(40),
            updated_at: "2026-08-18T12:00:00Z".to_string(),
            comment_count: 3,
            review_count: 2,
            head_commit_message: "msg".to_string(),
            author_name: "a".to_string(),
            author_email: "a@x".to_string(),
            committer_name: "c".to_string(),
            committer_email: "c@x".to_string(),
            pr_author_login: "a".to_string(),
        }
    }

    #[test]
    fn matches_identical_summary() {
        let pr = summary();
        let fp = SyncFingerprint::from_summary(&pr, 1000);
        assert!(fp.matches(&pr));
    }

    #[test]
    fn mismatch_on_each_field() {
        let pr = summary();
        let fp = SyncFingerprint::from_summary(&pr, 1000);

        let mut p = summary();
        p.updated_at = "2026-08-18T13:00:00Z".to_string();
        assert!(!fp.matches(&p));

        let mut p = summary();
        p.head_oid = "2".repeat(40);
        assert!(!fp.matches(&p));

        let mut p = summary();
        p.comment_count += 1;
        assert!(!fp.matches(&p));

        let mut p = summary();
        p.review_count += 1;
        assert!(!fp.matches(&p));
    }

    #[test]
    fn freshness_respects_ttl() {
        let fp = SyncFingerprint::from_summary(&summary(), 1000);
        assert!(fp.is_fresh(1000, 3600));
        assert!(fp.is_fresh(4600, 3600));
        assert!(!fp.is_fresh(4601, 3600));
        // Clock skew: fetched_at in the future counts as fresh.
        assert!(fp.is_fresh(500, 3600));
    }

    #[test]
    fn json_round_trip() {
        let fp = SyncFingerprint::from_summary(&summary(), 4242);
        let json = serde_json::to_string(&fp).unwrap();
        let back: SyncFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(fp, back);
    }
}
