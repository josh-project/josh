/// Pluggable storage layer for josh's per-filter, per-commit cache.
///
/// Each backend maps `(filter, from_oid) → to_oid`. The [`HistoryGraphHint`]
/// passed with every record lets backends like the distributed one shard or
/// skip records based on commit ordering and topology without reading the
/// commit from the object database.
pub trait CacheBackend: Send + Sync {
    fn read(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        hint: HistoryGraphHint,
    ) -> anyhow::Result<Option<git2::Oid>>;

    fn write(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        hint: HistoryGraphHint,
    ) -> anyhow::Result<()>;
}

/// Per-commit history-graph facts passed along with every cache record.
///
/// Both fields come from the cached hint maintained by the history-graph
/// walk, so producing them never requires reading the commit itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryGraphHint {
    pub sequence_number: u64,
    /// Number of parents of the commit, capped at 255. Backends only
    /// distinguish "exactly one parent" from merges (> 1) and orphans (0),
    /// so the cap is lossless for eligibility purposes.
    pub parent_count: u8,
}
