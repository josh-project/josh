//! Sync fingerprint cache on changes refs: snapshot of the list-query fields
//! from the last full comment fetch, so sync can skip unchanged PRs.
//! Persisted as a josh-git-serde tree at `gh_cache/<change-id>/fingerprint` --
//! the path is part of the on-disk format and must not change.

use josh_changes::ChangesRef;

use crate::layout::{GithubChangesRefData, SyncCache, GITHUB_CACHE_PATH};
use josh_core::cache::Transaction;
use josh_github_graphql::operations::pull_request::PrSummary;

use serde::{Deserialize, Serialize};

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
/// A missing or corrupt entry is a cache miss, not an error.
pub fn read_sync_fingerprint(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Option<SyncFingerprint>> {
    // Decode failures (e.g. pre-tree-format blobs) stay soft here: a corrupt
    // fingerprint is a cache miss, and sync refetches and overwrites.
    let data = josh_changes::read_filtered::<GithubChangesRefData>(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_CACHE_PATH),
    )
    .unwrap_or(None);
    Ok(data.and_then(|d| d.gh_cache.get(change_id).map(|c| c.fingerprint.clone())))
}

/// Store the sync fingerprint for a change after a successful full fetch.
pub fn store_sync_fingerprint(
    transaction: &Transaction,
    change_id: &str,
    fingerprint: &SyncFingerprint,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let data = GithubChangesRefData {
        gh_cache: [(
            change_id.to_string(),
            SyncCache {
                fingerprint: fingerprint.clone(),
            },
        )]
        .into(),
        ..Default::default()
    };
    josh_changes::write_filtered(
        transaction,
        scope,
        josh_changes::namespace_filter(GITHUB_CACHE_PATH),
        &data,
        None,
        None,
    )?;
    Ok(())
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
}
