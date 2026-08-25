/// Pluggable storage layer for josh's `(filter, from_oid) → to_oid` cache.
///
/// The [`HistoryGraphHint`] passed with every record lets backends like the
/// distributed one shard or skip records based on commit ordering and topology
/// without reading the commit from the object database.
///
/// Most records are keyed by commit, with the hint describing that commit's own
/// history position. Tree-keyed records (the trigram index) key on a tree oid
/// shared across many commits; the hint then describes the commit being indexed,
/// so backends must not gate reads on the reading commit's eligibility -- see
/// [`crate::cache::distributed::DistributedCacheBackend`].
pub trait CacheBackend: Send + Sync {
    fn read(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
        hint: HistoryGraphHint,
        tree_keyed: bool,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>>;

    fn write(
        &self,
        filter: crate::filter::Filter,
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
        hint: HistoryGraphHint,
        tree_keyed: bool,
    ) -> anyhow::Result<()>;

    /// Register the start of a transaction against this backend. The default is a no-op; backends
    /// whose underlying store holds a process-exclusive lock (the sled backend) use this to open
    /// lazily and reference-count active transactions.
    fn begin(&self) {}

    /// Register the end of a transaction. Balances a prior [`CacheBackend::begin`]. When the last
    /// active transaction ends, a locking backend flushes and drops its handles, releasing the lock
    /// until the next [`CacheBackend::begin`] transparently reopens what it needs.
    fn end(&self) {}
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

/// One in every `SAMPLE_INTERVAL` commits is a sample point, by sequence number.
const SAMPLE_INTERVAL: u64 = 100;

impl HistoryGraphHint {
    /// Whether this commit is a point at which a backward walk can be pruned.
    ///
    /// Caches are meant to be sparse: not every mapping is persisted. Both the
    /// distributed backend (to stay small and fast to download) and the local
    /// cache (which only persists mappings for commits that produce new output,
    /// plus these samples) rely on the same invariant, so that a filter which
    /// drops most commits -- and therefore stores almost nothing on its own --
    /// still gets its walks pruned within a bounded number of steps.
    ///
    /// Sample points are 1% of all commits (sampled by sequence number) plus
    /// every commit the sampling alone cannot bound: orphans (a walk path ends
    /// there with no parent whose entry could terminate it), octopus merges (the
    /// hint only covers the first two parents), and two-parent merges whose
    /// backward jump skips over a sample point. A single-parent step always
    /// decreases the sequence number by exactly 1, so any walk path reaches a
    /// sample point within at most [`SAMPLE_INTERVAL`] steps: either it takes
    /// that many consecutive unit steps and crosses a multiple of the interval,
    /// or it hits one of the jump/end commits first.
    ///
    /// Everything needed comes from the hint, so this check never reads the
    /// commit from the ODB.
    pub fn is_sample_point(self) -> bool {
        if self.sequence_number % SAMPLE_INTERVAL == 0 {
            return true;
        }
        match self.parent_count {
            1 => false,
            2 => crosses_sample_boundary(self.sequence_number, self.jump_delta),
            _ => true,
        }
    }
}

// True when a backward step of `delta` from `seq` skips over a multiple of
// SAMPLE_INTERVAL, i.e. the sampled commit that would otherwise bound the walk on
// this path is jumped over. `delta` saturates at 127, meaning "at least 127": any
// jump of SAMPLE_INTERVAL or more crosses a boundary, so the saturated value still
// decodes correctly.
fn crosses_sample_boundary(seq: u64, delta: u8) -> bool {
    seq.saturating_sub(delta as u64) / SAMPLE_INTERVAL < seq / SAMPLE_INTERVAL
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(sequence_number: u64, parent_count: u8, jump_delta: u8) -> HistoryGraphHint {
        HistoryGraphHint {
            sequence_number,
            parent_count,
            jump_delta,
            jump_is_second: false,
        }
    }

    // Regular single-parent commits are sampled by sequence number alone.
    #[test]
    fn samples_by_sequence_number() {
        assert!(hint(0, 1, 1).is_sample_point());
        assert!(hint(SAMPLE_INTERVAL, 1, 1).is_sample_point());
        assert!(!hint(SAMPLE_INTERVAL + 1, 1, 1).is_sample_point());
    }

    // Orphans end a walk path and octopus merges reach past the two parents the hint
    // covers, so neither can be bounded by sampling and both are always stored.
    #[test]
    fn samples_unbounded_topologies() {
        assert!(hint(SAMPLE_INTERVAL + 1, 0, 1).is_sample_point());
        assert!(hint(SAMPLE_INTERVAL + 1, 3, 1).is_sample_point());
    }

    // A two-parent merge is stored exactly when its backward jump would skip over the
    // sample point that would otherwise bound the walk on that path.
    #[test]
    fn samples_merges_that_jump_a_boundary() {
        assert!(!hint(SAMPLE_INTERVAL + 5, 2, 4).is_sample_point());
        assert!(hint(SAMPLE_INTERVAL + 5, 2, 6).is_sample_point());
        // The delta saturates at 127; any jump that long crosses a boundary regardless.
        assert!(hint(SAMPLE_INTERVAL + 5, 2, 127).is_sample_point());
    }
}
