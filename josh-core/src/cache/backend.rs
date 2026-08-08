/// Pluggable storage layer for josh's per-filter cache.
///
/// Each backend maps `(filter, from_oid) → to_oid`. The [`HistoryGraphHint`]
/// passed with every record lets backends like the distributed one shard or
/// skip records based on commit ordering and topology without reading the
/// commit from the object database.
///
/// Most records are per-commit: `from` is a commit and the hint describes that
/// commit's own position in history. Records with `tree_keyed == true` (the
/// trigram index) instead key on a *tree* oid shared across many commits, and
/// the hint describes the commit currently being indexed rather than the key.
/// Because the key is content-shared, the writing and reading commits differ,
/// so a backend must not gate reads on the reading commit's eligibility -- see
/// [`crate::cache::distributed::DistributedCacheBackend`].
pub trait CacheBackend: Send + Sync {
    fn read(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        hint: HistoryGraphHint,
        tree_keyed: bool,
    ) -> anyhow::Result<Option<git2::Oid>>;

    fn write(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        hint: HistoryGraphHint,
        tree_keyed: bool,
    ) -> anyhow::Result<()>;

    /// Drop any cached handles into the underlying store. The default is a no-op; backends holding
    /// a lock (e.g. the sled tree handles) override this so the lock can be released. Reads and
    /// writes stay valid afterwards, transparently reacquiring what they need.
    fn release(&self) {}
}

/// Per-commit history-graph facts passed along with every cache record.
///
/// All fields come from the cached hint maintained by the history-graph
/// walk, so producing them never requires reading the commit itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryGraphHint {
    pub sequence_number: u64,
    /// Number of parents of the commit, capped at 255. Backends only
    /// distinguish "exactly one parent" from two-parent merges, octopus
    /// merges (> 2) and orphans (0), so the cap is lossless for eligibility
    /// purposes.
    pub parent_count: u8,
    /// Sequence-number distance to the farther of the first two parents,
    /// saturating at 127 ("at least 127"). The sequence number is
    /// `max(parents) + 1`, so for single-parent commits this is always 1 and
    /// for two-parent merges the *other* parent is always at distance exactly
    /// 1 — this value carries the only non-trivial distance. Parents beyond
    /// the first two are not covered (octopus merges are handled
    /// unconditionally by eligibility).
    pub jump_delta: u8,
    /// True when the parent measured by `jump_delta` is the second parent.
    pub jump_is_second: bool,
}
