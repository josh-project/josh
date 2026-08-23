//! Serde view of the changes-ref tree layout.
//!
//! Serialized maps are trees keyed by entry name, so a ref tree narrowed by
//! `namespace_filter` deserializes directly into the struct below; readers
//! select the populated field. Writes still place values by path.
//!
//! Comments are absent: the file-comment branch embeds the file's blob id
//! and path components between the change id and the comment hash, which a
//! map cannot key. Typing comments needs a layout change first.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{DiffData, VoteData};

/// change id → user → vote.
pub type VotesByChange = HashMap<String, HashMap<String, VoteData>>;

/// change id → diff metadata.
pub type DiffsByChange = HashMap<String, DiffData>;

/// Writes queued locally, awaiting forge publication. Mirrors the
/// corresponding top-level namespaces.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Outbox {
    #[serde(default)]
    pub votes: VotesByChange,
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
    pub outbox: Outbox,
}
