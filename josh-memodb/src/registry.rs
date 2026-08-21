//! The process-wide map from objects directory to [`MemOdb`].
//!
//! A store has to outlive the transactions writing to it, or packing is back on transaction
//! lifetime. Keyed by objects directory rather than repository path because a linked worktree, a
//! bare clone and a gitdir file all reach the same objects through different paths. Entries are
//! held strongly and never removed; a drained store is two empty collections.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use crate::MemOdb;

static STORES: LazyLock<Mutex<HashMap<PathBuf, Arc<MemOdb>>>> = LazyLock::new(Default::default);

/// A registered store lives as long as the process, so it always has a ceiling even when the
/// caller asks for none. Matches the limit josh-cli and josh-proxy ask for explicitly.
const DEFAULT_LIMIT: usize = 128 * 1024 * 1024;

/// The store for `objects_dir`, created on first use. `limit` is a budget for the objects
/// directory, not for one transaction's buffer, so it only ever tightens (see
/// [`MemOdb::tighten_limit`]).
pub fn shared(limit: Option<usize>, objects_dir: &Path) -> Arc<MemOdb> {
    let limit = Some(limit.unwrap_or(DEFAULT_LIMIT));
    // Two handles can name the same objects directory differently (a relative path, a symlinked
    // checkout); resolving keeps them on one store. An unresolvable path is used as given.
    let key = std::fs::canonicalize(objects_dir).unwrap_or_else(|_| objects_dir.to_path_buf());

    let mut stores = STORES.lock().unwrap();
    let store = stores
        .entry(key)
        .or_insert_with(|| MemOdb::build(limit, objects_dir.to_path_buf(), None))
        .clone();
    store.tighten_limit(limit);
    store
}

/// Flushes every registered store to disk when dropped. Keep one guard alive for the lifetime of
/// a binary so every return path, including errors and unwinding panics, drains the process-wide
/// stores.
#[must_use = "dropping the guard immediately flushes all registered stores"]
pub struct FlushGuard;

impl FlushGuard {
    pub fn new() -> Self {
        Self
    }
}

impl Drop for FlushGuard {
    fn drop(&mut self) {
        flush_all();
    }
}

/// Pack every registered store to disk, inline.
fn flush_all() {
    let stores: Vec<Arc<MemOdb>> = STORES.lock().unwrap().values().cloned().collect();
    for store in stores {
        if let Err(e) = store.pack_to_disk() {
            log::error!("failed to flush in-memory object store at exit: {e}");
        }
    }
}
