mod backend;
pub mod distributed;
mod history_graph;
pub mod sled;
pub mod stack;
mod transaction;

/// Schema version for on-disk cache structures. Bump when the layout of the
/// sled trees or distributed cache refs changes incompatibly.
pub const CACHE_VERSION: u64 = 30;

pub use backend::{CacheBackend, HistoryGraphHint};
pub use distributed::DistributedCacheBackend;
pub use history_graph::{
    HistoryGraphInfo, collect_history_graph_info, compute_history_hint, compute_sequence_number,
    parents_share_root,
};
pub use sled::{SledCacheBackend, sled_clear, sled_load, sled_open_josh_trees, sled_print_stats};
pub use stack::CacheStack;
pub use transaction::*;
