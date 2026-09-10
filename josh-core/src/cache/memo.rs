use crate::filter::{DownstackDep, Filter};
use gix_hash::ObjectId;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Independent borrow state lets unrelated memo tables be used concurrently.
pub(crate) struct MemoMap<K, V> {
    entries: RefCell<HashMap<K, V>>,
}

impl<K, V> Default for MemoMap<K, V> {
    fn default() -> Self {
        Self {
            entries: Default::default(),
        }
    }
}

impl<K: Eq + Hash, V: Clone> MemoMap<K, V> {
    pub(crate) fn get(&self, key: &K) -> Option<V> {
        self.entries.borrow().get(key).cloned()
    }

    pub(crate) fn get_with<R>(&self, key: &K, f: impl FnOnce(&V) -> R) -> Option<R> {
        self.entries.borrow().get(key).map(f)
    }

    pub(crate) fn insert(&self, key: K, value: V) {
        self.entries.borrow_mut().insert(key, value);
    }

    pub(crate) fn insert_if_absent(&self, key: K, value: V) {
        self.entries.borrow_mut().entry(key).or_insert(value);
    }
}

/// Partitions entries by a first key to avoid hashing composite keys and allocating unused
/// inner maps.
pub(crate) struct PartitionedMemoMap<P, K, V> {
    entries: RefCell<HashMap<P, HashMap<K, V>>>,
}

impl<P, K, V> Default for PartitionedMemoMap<P, K, V> {
    fn default() -> Self {
        Self {
            entries: Default::default(),
        }
    }
}

impl<P: Eq + Hash, K: Eq + Hash, V: Clone> PartitionedMemoMap<P, K, V> {
    pub(crate) fn get(&self, partition: &P, key: &K) -> Option<V> {
        self.entries
            .borrow()
            .get(partition)
            .and_then(|entries| entries.get(key))
            .cloned()
    }

    pub(crate) fn insert(&self, partition: P, key: K, value: V) {
        self.entries
            .borrow_mut()
            .entry(partition)
            .or_default()
            .insert(key, value);
    }
}

#[derive(Default)]
pub(crate) struct TransactionMemo {
    pub(crate) apply: PartitionedMemoMap<ObjectId, ObjectId, ObjectId>,
    pub(crate) subtract: MemoMap<(ObjectId, ObjectId), ObjectId>,
    pub(crate) intersect: MemoMap<(ObjectId, ObjectId), ObjectId>,
    pub(crate) overlay: MemoMap<(ObjectId, ObjectId), ObjectId>,
    pub(crate) unapply: PartitionedMemoMap<ObjectId, ObjectId, ObjectId>,
    pub(crate) legalize: MemoMap<(Filter, ObjectId), Filter>,
    pub(crate) downstack_deps: MemoMap<ObjectId, HashSet<DownstackDep>>,
    pub(crate) merge_trees: MemoMap<(ObjectId, ObjectId, ObjectId), ObjectId>,
    pub(crate) references: PartitionedMemoMap<ObjectId, ObjectId, ObjectId>,
    pub(crate) populate: MemoMap<(ObjectId, ObjectId), ObjectId>,
    /// Keyed by (input tree, pattern key, NFA state mask). The state mask makes entries
    /// independent of the path used to reach a subtree. Full-path lookups fold their root into
    /// a synthetic pattern key and use mask 0.
    pub(crate) glob: MemoMap<(ObjectId, ObjectId, u64), ObjectId>,
    /// Path-projection memoization for `:PATHS` and its inverse, keyed by (input tree oid,
    /// root path). Workspace filters walk commits parent-first, so a child commit reuses the
    /// projections its parent computed for shared subtrees.
    pub(crate) paths: MemoMap<(ObjectId, String), ObjectId>,
    pub(crate) invert: MemoMap<(ObjectId, String), ObjectId>,
    pub(crate) workspaces: MemoMap<ObjectId, Filter>,
    pub(crate) ancestors: MemoMap<ObjectId, HashSet<ObjectId>>,
    /// The most recent commit josh wrote as `(oid, tree_id)`. A history walk processes a commit
    /// immediately after its parent, so one slot avoids reparsing without retaining every commit.
    pub(crate) last_written_commit: Cell<Option<(ObjectId, ObjectId)>>,
}
