/// Pluggable storage layer for josh's per-filter, per-commit cache.
///
/// Each backend maps `(filter, from_oid, sequence_number) → to_oid`. The
/// `sequence_number` lets backends like the distributed one shard or skip
/// records based on commit ordering.
pub trait CacheBackend: Send + Sync {
    fn read(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        sequence_number: u64,
    ) -> anyhow::Result<Option<git2::Oid>>;

    fn write(
        &self,
        filter: crate::filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        sequence_number: u64,
    ) -> anyhow::Result<()>;
}
