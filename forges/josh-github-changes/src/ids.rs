//! GitHub ID tracking on changes refs: which local comment or vote maps to
//! which GitHub node ID. Persisted as blobs under `gh_ids/<change-id>/...` and
//! `gh_vote_ids/<change-id>/...` inside the changes ref — the paths are part
//! of the on-disk format and must not change.

use std::collections::HashMap;

use josh_changes::{encode_change_id_path, ChangesRef, VoteData};
use josh_core::cache::Transaction;

/// Store a GitHub node ID for a local comment, marking it as posted.
pub fn store_github_id(
    transaction: &Transaction,
    change_id: &str,
    local_hash: &str,
    github_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let blob_oid = josh_core::objects::write_blob(transaction.odb(), github_id.as_bytes())?;
    let path = std::path::Path::new("gh_ids")
        .join(encode_change_id_path(change_id))
        .join(local_hash);
    josh_changes::write_changes_tree(transaction, &path, blob_oid, None, None, scope)?;
    Ok(())
}

/// Read all GitHub node IDs for a change's comments.
/// Returns a map from local comment hash → GitHub node ID.
pub fn read_github_ids(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<HashMap<String, String>> {
    let path = std::path::Path::new("gh_ids").join(encode_change_id_path(change_id));
    Ok(josh_changes::read_blob_map(transaction, scope, &path)?
        .into_iter()
        .map(|(name, id)| (name, id.trim().to_string()))
        .collect())
}

/// Record a vote as posted to GitHub for the given user.
pub fn store_github_vote_id(
    transaction: &Transaction,
    change_id: &str,
    user: &str,
    vote_data: &VoteData,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(vote_data)?;
    let blob_oid = josh_core::objects::write_blob(transaction.odb(), json.as_bytes())?;
    let path = std::path::Path::new("gh_vote_ids")
        .join(encode_change_id_path(change_id))
        .join(user);
    josh_changes::write_changes_tree(transaction, &path, blob_oid, None, None, scope)?;
    Ok(())
}

/// Read the votes recorded as posted to GitHub for a change.
/// Returns a map from user → vote data.
pub fn read_github_vote_ids(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<HashMap<String, VoteData>> {
    let path = std::path::Path::new("gh_vote_ids").join(encode_change_id_path(change_id));
    Ok(josh_changes::read_blob_map(transaction, scope, &path)?
        .into_iter()
        .filter_map(|(user, json)| {
            serde_json::from_str::<VoteData>(&json)
                .ok()
                .map(|d| (user, d))
        })
        .collect())
}
