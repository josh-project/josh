//! The two in-memory caches (both cleared via `josh_wasm::clear_caches`):
//!
//! - a bounded LRU of validated/compiled modules keyed by blob OID. Bounded
//!   because each entry holds a compiled module of up to the module size limit
//!   and anyone who can push to a proxied repo can mint unlimited distinct
//!   blob OIDs.
//! - an evaluation memo keyed by `(module blob OID, args, filtered tree OID)`.
//!   Deliberately unbounded: entries are a few dozen bytes (OIDs plus an
//!   interned `Filter` pointer), the same growth class as josh-core's
//!   `WORKSPACES` map.

use crate::engine::CompiledModule;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

pub(crate) type EvalMemoKey = (gix_hash::ObjectId, Vec<String>, gix_hash::ObjectId);

/// Hand-rolled LRU: last-use tick per entry, evict the smallest tick on
/// insert at capacity. The O(n) eviction scan is fine at the default n=64.
pub(crate) struct ModuleLru {
    capacity: usize,
    tick: u64,
    entries: HashMap<gix_hash::ObjectId, (Arc<dyn CompiledModule>, u64)>,
}

impl ModuleLru {
    pub(crate) fn new(capacity: usize) -> ModuleLru {
        ModuleLru {
            capacity,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, oid: &gix_hash::ObjectId) -> Option<Arc<dyn CompiledModule>> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(oid).map(|entry| {
            entry.1 = tick;
            entry.0.clone()
        })
    }

    pub(crate) fn insert(&mut self, oid: gix_hash::ObjectId, module: Arc<dyn CompiledModule>) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&oid) {
            if let Some(evict) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, tick))| *tick)
                .map(|(oid, _)| *oid)
            {
                tracing::trace!("evicting wasm module {} from cache", evict);
                self.entries.remove(&evict);
            }
        }
        self.tick += 1;
        self.entries.insert(oid, (module, self.tick));
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, oid: &gix_hash::ObjectId) -> bool {
        self.entries.contains_key(oid)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

static MODULE_CACHE: LazyLock<Mutex<ModuleLru>> =
    LazyLock::new(|| Mutex::new(ModuleLru::new(*crate::JOSH_WASM_MODULE_CACHE_SIZE)));

static EVAL_MEMO: LazyLock<Mutex<HashMap<EvalMemoKey, josh_filter::Filter>>> =
    LazyLock::new(Default::default);

pub(crate) fn module_cache() -> &'static Mutex<ModuleLru> {
    &MODULE_CACHE
}

pub(crate) fn eval_memo() -> &'static Mutex<HashMap<EvalMemoKey, josh_filter::Filter>> {
    &EVAL_MEMO
}

pub(crate) fn clear() {
    MODULE_CACHE.lock().clear();
    EVAL_MEMO.lock().clear();
}

/// Number of memo entries for a given module blob OID. Test helper: global
/// cache state is shared across parallel tests, so assertions must be scoped
/// to a module OID unique to the asserting test.
#[cfg(test)]
pub(crate) fn eval_memo_entries_for(module_oid: gix_hash::ObjectId) -> usize {
    EVAL_MEMO
        .lock()
        .keys()
        .filter(|(oid, _, _)| *oid == module_oid)
        .count()
}
