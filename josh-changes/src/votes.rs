use std::collections::HashMap;

use crate::change::{Change, encode_change_id_path};
use crate::layout::ChangesRefData;
use crate::refs::ChangesRef;
use crate::store::{namespace_filter, read_filtered};

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

    let data = VoteData {
        state: state.to_string(),
        sha: change.commit().to_string(),
    };

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

    let root = crate::store::write_value(transaction, &path, &data, author, timestamp, scope)?;
    Ok(root.to_string())
}

pub fn read_vote(
    transaction: &Transaction,
    change_id: &str,
    user: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<Option<VoteData>> {
    let user = match user {
        Some(name) => name.to_string(),
        None => transaction
            .signature()?
            .email()
            .unwrap_or("unknown")
            .to_string(),
    };

    let Some(data) =
        read_filtered::<ChangesRefData>(transaction, scope, namespace_filter("votes"))?
    else {
        return Ok(None);
    };
    Ok(data
        .votes
        .get(change_id)
        .and_then(|votes| votes.get(&user))
        .cloned())
}

pub fn list_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let data = read_filtered::<ChangesRefData>(transaction, scope, namespace_filter("votes"))?;
    Ok(sorted_votes(
        data.as_ref().and_then(|d| d.votes.get(change_id)),
    ))
}

/// List votes queued in the outbox subtree of `scope` (must be Remote in
/// practice; this just returns empty for refs that lack `outbox/votes`).
pub fn list_outbox_votes(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let data =
        read_filtered::<ChangesRefData>(transaction, scope, namespace_filter("outbox/votes"))?;
    Ok(sorted_votes(
        data.as_ref().and_then(|d| d.outbox.votes.get(change_id)),
    ))
}

/// Map entries as a list sorted by user: tree iteration order, which the
/// previous manual walk produced.
fn sorted_votes(votes: Option<&HashMap<String, VoteData>>) -> Vec<(String, VoteData)> {
    let mut votes: Vec<_> = votes
        .map(|m| m.iter().map(|(u, v)| (u.clone(), v.clone())).collect())
        .unwrap_or_default();
    votes.sort_by(|a, b| a.0.cmp(&b.0));
    votes
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
            tree = tree::insert_oid(
                odb,
                tree,
                &path,
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                0,
            )?;
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
