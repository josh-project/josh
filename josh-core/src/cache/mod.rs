mod backend;
pub mod distributed;
mod history_graph;
pub mod sled;
pub mod stack;
mod transaction;
mod tree_cache;

/// Schema version for on-disk cache structures. Bump when the layout of the
/// sled trees or distributed cache refs changes incompatibly.
/// Version 33: history hints gained the jump-delta byte and jump-parent flag;
/// old hints would decode as jump_delta 0 and never classify as a boundary
/// crossing.
pub const CACHE_VERSION: u64 = 33;

pub use backend::{CacheBackend, HistoryGraphHint};
pub use distributed::DistributedCacheBackend;
pub use history_graph::{
    HistoryGraphInfo, collect_history_graph_info, compute_history_hint, compute_sequence_number,
    parents_share_root,
};
pub use sled::{SledCacheBackend, sled_clear, sled_print_stats};
pub use stack::CacheStack;
pub use transaction::*;
pub use tree_cache::TreeBytes;
