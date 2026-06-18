mod backend;
pub mod distributed;
mod history_graph;
mod mem_odb;
pub mod sled;
pub mod stack;
mod transaction;

/// Schema version for on-disk cache structures. Bump when the layout of the
/// sled trees or distributed cache refs changes incompatibly.
pub const CACHE_VERSION: u64 = 27;

pub use backend::CacheBackend;
pub use distributed::DistributedCacheBackend;
pub use history_graph::{
    HistoryGraphInfo, collect_history_graph_info, compute_sequence_number, parents_share_root,
};
pub use sled::{SledCacheBackend, sled_clear, sled_load, sled_open_josh_trees, sled_print_stats};
pub use stack::CacheStack;
pub use transaction::*;

pub(crate) use mem_odb::flush_all_at;

/// Flush in-memory objects in `repo`'s ODB backend to a packfile on disk. Call before handing the
/// repository to an external `git` process, which cannot see the in-process in-memory backend.
pub fn flush_objects(repo: &git2::Repository) -> anyhow::Result<()> {
    mem_odb::flush_all(repo)?;
    Ok(())
}
