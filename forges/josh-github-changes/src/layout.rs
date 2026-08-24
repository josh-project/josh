//! Serde view of the GitHub-owned namespaces on a changes ref.
//!
//! Like `josh_changes::layout`, but for the `gh*` subtrees: reads scope a ref
//! to a subtree with a josh filter, then deserialize; writes populate a
//! sparse struct and merge it back with `josh_changes::write_filtered`.
//! Field names are the literal top-level tree entries.

use std::collections::HashMap;

use josh_changes::VoteData;
use josh_github_graphql::operations::pull_request::PrData;
use serde::{Deserialize, Serialize};

use crate::SyncFingerprint;

pub const GITHUB_COMMENT_NODE_IDS_PATH: &str = "gh_comment_node_ids";
pub const GITHUB_VOTE_NODE_IDS_PATH: &str = "gh_vote_node_ids";

/// Path of the sync-fingerprint cache namespace.
pub const GITHUB_CACHE_PATH: &str = "gh_cache";

/// Path of the stored pull-request data namespace.
pub const GITHUB_PR_DATA_PATH: &str = "gh";

/// local comment hash → GitHub GraphQL node id.
pub type CommentNodeIds = HashMap<String, String>;

/// change id → comment node ids.
pub type CommentNodeIdsByChange = HashMap<String, CommentNodeIds>;

/// user → vote recorded as posted.
pub type VoteNodeIds = HashMap<String, VoteData>;

/// change id → posted votes per user.
pub type VoteNodeIdsByChange = HashMap<String, VoteNodeIds>;

/// change id → cached PR metadata (`gh/`).
pub type PrDataByChange = HashMap<String, PrData>;

/// change id → sync cache (`gh_cache/`).
pub type CacheByChange = HashMap<String, SyncCache>;

/// Per-change sync cache entries.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncCache {
    pub fingerprint: SyncFingerprint,
}

/// GitHub-owned namespaces of a changes ref.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GithubChangesRefData {
    #[serde(default)]
    pub gh_comment_node_ids: CommentNodeIdsByChange,
    #[serde(default)]
    pub gh_vote_node_ids: VoteNodeIdsByChange,
    #[serde(default)]
    pub gh: PrDataByChange,
    #[serde(default)]
    pub gh_cache: CacheByChange,
}
