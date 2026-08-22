use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use crate::store::{get_tree, parse_timestamp};
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::objects;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteData {
    pub state: String,
    pub sha: String,
}

pub fn write_vote(
    transaction: &Transaction,
    change: &Change,
    state: &str,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<String> {
    write_vote_inner(
        transaction,
        change,
        state,
        author,
        timestamp,
        scope,
        "votes",
    )
}

/// Write a vote into the outbox subtree of a `Remote` ref. The vote is queued
/// for the next `sync --push` to post to the forge, after which the forge's
/// posted-vote tracking records the post and the outbox entry can be cleaned
/// up.
pub fn write_outbox_vote(
    transaction: &Transaction,
    change: &Change,
    state: &str,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<String> {
    if !matches!(scope, ChangesRef::Remote { .. }) {
        return Err(anyhow::anyhow!(
            "write_outbox_vote requires a Remote scope (got {})",
            scope.ref_name()
        ));
    }
    write_vote_inner(
        transaction,
        change,
        state,
        author,
        timestamp,
        scope,
        "outbox/votes",
    )
}

fn write_vote_inner(
    transaction: &Transaction,
    change: &Change,
    state: &str,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
    path_prefix: &str,
) -> anyhow::Result<String> {
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let json = serde_json::json!({"state": state, "sha": change.commit().to_string()});
    let content = json.to_string();
    let content_hash =
        git2::Oid::hash_object(git2::ObjectType::Blob, content.as_bytes())?.to_string();
    let odb = transaction.odb();
    let blob_oid = objects::write_blob(odb, content.as_bytes())?;

    let tree_oid = tree::insert_oid(
        odb,
        tree::empty_id(),
        std::path::Path::new(&blob_oid.to_string()),
        blob_oid,
        git2::FileMode::Blob.into(),
    )?;

    let user = match author {
        Some(name) => name.to_string(),
        None => transaction
            .signature()?
            .email()
            .unwrap_or("unknown")
            .to_string(),
    };

    let path = std::path::Path::new(path_prefix)
        .join(encode_change_id_path(&change_id))
        .join(&user);

    let ref_name = scope.ref_name();
    let prev_commit = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_commit {
        Some(oid) => objects::CommitData::read(odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    if let Ok(Some(existing)) = tree::get_path_entry(transaction, odb, base_tree, &path) {
        if objects::git2_oid(&existing.oid) == tree_oid {
            return Ok(content_hash);
        }
    }

    let tree = tree::insert_oid(odb, base_tree, &path, tree_oid, 0o0040000)?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => transaction.signature()?,
    };
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(odb, tree, prev_commit.as_slice(), &sig, &sig, &msg)?;
    transaction.update_ref(
        &ref_name,
        prev_commit.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;

    Ok(content_hash)
}

pub fn read_vote(
    transaction: &Transaction,
    change_id: &str,
    user: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<Option<VoteData>> {
    let odb = transaction.odb();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => objects::CommitData::read(odb, oid)?.tree_id()?,
        None => return Ok(None),
    };

    let user = match user {
        Some(name) => name.to_string(),
        None => transaction
            .signature()?
            .email()
            .unwrap_or("unknown")
            .to_string(),
    };

    let path = std::path::Path::new("votes")
        .join(encode_change_id_path(change_id))
        .join(&user);

    let subtree = match get_tree(transaction, odb, tree, &path) {
        Some(t) => t,
        None => return Ok(None),
    };
    for entry in tree::read_tree(transaction, odb, subtree)?.entries() {
        if let Some(blob) = tree::blob_bytes(odb, objects::git2_oid(&entry.oid)) {
            let data: VoteData = serde_json::from_slice(&blob)?;
            return Ok(Some(data));
        }
    }
    Ok(None)
}

pub fn list_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    list_votes_at_prefix(transaction, change_id, scope, "votes")
}

/// List votes queued in the outbox subtree of `scope` (must be Remote in
/// practice; this just returns empty for refs that lack `outbox/votes`).
pub fn list_outbox_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    list_votes_at_prefix(transaction, change_id, scope, "outbox/votes")
}

fn list_votes_at_prefix(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    path_prefix: &str,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let odb = transaction.odb();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => objects::CommitData::read(odb, oid)?.tree_id()?,
        None => return Ok(Default::default()),
    };
    let path = std::path::Path::new(path_prefix).join(encode_change_id_path(change_id));
    let subtree = match get_tree(transaction, odb, tree, &path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut votes = Vec::new();
    for entry in tree::read_tree(transaction, odb, subtree)?.entries() {
        let user = match std::str::from_utf8(entry.filename) {
            Ok(name) => name.to_string(),
            Err(_) => continue,
        };
        if !entry.mode.is_tree() {
            continue;
        }
        let user_tree = match tree::read_tree(transaction, odb, objects::git2_oid(&entry.oid)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for child in user_tree.entries() {
            if let Some(blob) = tree::blob_bytes(odb, objects::git2_oid(&child.oid)) {
                if let Ok(data) = serde_json::from_slice::<VoteData>(&blob) {
                    votes.push((user.clone(), data));
                }
            }
        }
    }
    Ok(votes)
}

/// Delete the outbox vote entries of the given users from `scope`'s ref.
/// Returns the number of entries removed.
pub fn delete_outbox_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    users: &[String],
) -> anyhow::Result<usize> {
    if users.is_empty() {
        return Ok(0);
    }

    let odb = transaction.odb();
    let encoded = encode_change_id_path(change_id);
    let ref_name = scope.ref_name();
    let prev_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => oid,
        None => return Ok(0),
    };
    let mut tree = objects::CommitData::read(odb, prev_commit)?.tree_id()?;

    let mut removed = 0usize;
    for user in users {
        let path = std::path::Path::new("outbox/votes")
            .join(&encoded)
            .join(user);
        if matches!(
            tree::get_path_entry(transaction, odb, tree, &path),
            Ok(Some(_))
        ) {
            tree = tree::insert_oid(odb, tree, &path, git2::Oid::ZERO_SHA1, 0)?;
            removed += 1;
        }
    }

    if removed == 0 {
        return Ok(0);
    }

    let sig = transaction.signature()?;
    let msg = format!("delete outbox votes on {}\n", ref_name);
    let new_oid = objects::write_commit(odb, tree, &[prev_commit], &sig, &sig, &msg)?;
    transaction.update_ref(&ref_name, Expected::At(prev_commit), new_oid, &msg)?;

    Ok(removed)
}
