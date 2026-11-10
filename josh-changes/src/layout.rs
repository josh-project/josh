//! Serde view of the changes-ref tree layout.
//!
//! Serialized maps are trees keyed by entry name, so a ref tree narrowed by
//! `namespace_filter` deserializes directly into the struct below; readers
//! select the populated field. Writes populate a sparse struct and merge it
//! back through the same filter (see `store::write_filtered`).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::comments::CommentMeta;
use crate::{DiffData, VoteData};

/// change id → user → vote.
pub type VotesByChange = HashMap<String, HashMap<String, VoteData>>;

/// The diffs namespace name.
pub(crate) const DIFFS_PATH: &str = "diffs";

/// change id → diff metadata.
pub type DiffsByChange = HashMap<String, DiffData>;

/// change id → comment tree id → comment. The comment id is the tree id of
/// the serialized `CommentMeta` it names.
pub type CommentsByChange = HashMap<String, HashMap<String, CommentMeta>>;

/// Writes queued locally, awaiting forge publication. Mirrors the
/// corresponding top-level namespaces.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Outbox {
    #[serde(default)]
    pub votes: VotesByChange,
    #[serde(default)]
    pub comments: CommentsByChange,
}

/// Core namespaces of a changes ref. Field names are the literal top-level
/// tree entries.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ChangesRefData {
    #[serde(default)]
    pub votes: VotesByChange,
    #[serde(default)]
    pub diffs: DiffsByChange,
    #[serde(default)]
    pub comments: CommentsByChange,
    #[serde(default)]
    pub outbox: Outbox,
}

/// A `refs/heads` namespace in the store-wide change index.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct Heads<T> {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub heads: BTreeMap<String, T>,
}

/// The literal `refs/heads` nesting used below projections.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct RefNamespace<T> {
    pub refs: Heads<T>,
}

/// One logical change in the store-wide index.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SuperChangeData {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projections: BTreeMap<String, RefNamespace<DiffData>>,
}

/// Serde view of the store-wide `refs/josh/super:changes` layout.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SuperRefData {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, SuperChangeData>,
}

impl SuperRefData {
    pub fn projection(change_id: &str, remote: &str, branch: &str, revision: DiffData) -> Self {
        let mut heads = BTreeMap::new();
        heads.insert(branch.to_owned(), revision);
        let mut projections = BTreeMap::new();
        projections.insert(
            remote.to_owned(),
            RefNamespace {
                refs: Heads { heads },
            },
        );
        let mut changes = BTreeMap::new();
        changes.insert(
            change_id.to_owned(),
            SuperChangeData {
                projections,
                ..Default::default()
            },
        );
        Self { changes }
    }
}
