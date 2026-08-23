//! Comments and votes are considered "posted" -- written to remote --
//! when the ID is stored in the tree. The ID is the GraphQL node id
//! returned by GitHub during create operations.

use josh_changes::{encode_change_id_path, namespace_filter, ChangesRef, VoteData};
use josh_core::cache::Transaction;

use crate::layout::{
    CommentNodeIds, GithubChangesRefData, VoteNodeIds, GITHUB_COMMENT_NODE_IDS_PATH,
    GITHUB_VOTE_NODE_IDS_PATH,
};

/// Store a GitHub node ID for a local comment, marking it as posted.
pub fn store_comment_node_id(
    transaction: &Transaction,
    change_id: &str,
    local_hash: &str,
    github_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let blob_oid = josh_core::objects::write_blob(transaction.odb(), github_id.as_bytes())?;
    let path = std::path::Path::new(GITHUB_COMMENT_NODE_IDS_PATH)
        .join(encode_change_id_path(change_id))
        .join(local_hash);

    josh_changes::write_changes_tree(transaction, &path, blob_oid, None, None, scope)?;
    Ok(())
}

/// Read all GitHub node IDs for a change's comments.
/// Returns a map from local comment hash → GitHub node ID.
pub fn read_comment_node_ids(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<CommentNodeIds> {
    let Some(data) = josh_changes::read_filtered::<GithubChangesRefData>(
        transaction,
        scope,
        namespace_filter(GITHUB_COMMENT_NODE_IDS_PATH),
    )?
    else {
        return Ok(Default::default());
    };

    Ok(data
        .gh_comment_node_ids
        .get(change_id)
        .cloned()
        .unwrap_or_default())
}

/// Record a vote as posted to GitHub for the given user.
pub fn store_vote_node_id(
    transaction: &Transaction,
    change_id: &str,
    user: &str,
    vote_data: &VoteData,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let path = std::path::Path::new(GITHUB_VOTE_NODE_IDS_PATH)
        .join(encode_change_id_path(change_id))
        .join(user);

    josh_changes::write_value(transaction, &path, vote_data, None, None, scope)?;
    Ok(())
}

/// Read the votes recorded as posted to GitHub for a change.
/// Returns a map from user → vote data.
pub fn read_vote_node_ids(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<VoteNodeIds> {
    let Some(data) = josh_changes::read_filtered::<GithubChangesRefData>(
        transaction,
        scope,
        namespace_filter(GITHUB_VOTE_NODE_IDS_PATH),
    )?
    else {
        return Ok(Default::default());
    };

    Ok(data
        .gh_vote_node_ids
        .get(change_id)
        .cloned()
        .unwrap_or_default())
}
