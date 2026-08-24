//! Comments and votes are considered "posted" -- written to remote --
//! when the ID is stored in the tree. The ID is the GraphQL node id
//! returned by GitHub during create operations.

use josh_changes::{namespace_filter, ChangesRef, VoteData};
use josh_core::cache::Transaction;

use crate::layout::{
    CommentNodeIds, GithubChangesRefData, VoteNodeIds, GITHUB_COMMENT_NODE_IDS_PATH,
    GITHUB_VOTE_NODE_IDS_PATH,
};

/// Store GitHub node IDs for a change's local comments, marking them as
/// posted. One commit regardless of how many `entries` carry; an empty slice
/// writes nothing.
pub fn store_comment_node_ids(
    transaction: &Transaction,
    change_id: &str,
    entries: &[(String, String)],
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let data = GithubChangesRefData {
        gh_comment_node_ids: [(
            change_id.to_string(),
            entries.iter().cloned().collect::<CommentNodeIds>(),
        )]
        .into(),
        ..Default::default()
    };

    josh_changes::write_filtered(
        transaction,
        scope,
        namespace_filter(GITHUB_COMMENT_NODE_IDS_PATH),
        &data,
        None,
        None,
    )
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

/// Record votes as posted to GitHub for the given users. One commit
/// regardless of how many `entries` carry; an empty slice writes nothing.
pub fn store_vote_node_ids(
    transaction: &Transaction,
    change_id: &str,
    entries: &[(String, VoteData)],
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let data = GithubChangesRefData {
        gh_vote_node_ids: [(
            change_id.to_string(),
            entries.iter().cloned().collect::<VoteNodeIds>(),
        )]
        .into(),
        ..Default::default()
    };

    josh_changes::write_filtered(
        transaction,
        scope,
        namespace_filter(GITHUB_VOTE_NODE_IDS_PATH),
        &data,
        None,
        None,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use josh_core::cache::{CacheStack, SledCacheBackend, TransactionContext};

    fn open_transaction(td: &tempfile::TempDir) -> Transaction {
        gix::init_bare(td.path()).unwrap();
        // Commits need an identity; don't depend on the ambient global git
        // config (CI containers have none).
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .append(true)
            .open(td.path().join("config"))
            .unwrap()
            .write_all(b"\n[user]\n\tname = test\n\temail = test@example.com\n")
            .unwrap();

        let cachestack =
            std::sync::Arc::new(CacheStack::new().with_backend(SledCacheBackend::new(td.path())));
        TransactionContext::new(td.path(), cachestack)
            .open()
            .unwrap()
    }

    /// Sparse writes merge: each store call carries a single entry, and the
    /// overlay must preserve entries written by earlier calls -- siblings
    /// under the same change, other changes, and other namespaces.
    #[test]
    fn node_id_writes_merge_without_clobbering() {
        let td = tempfile::tempdir().unwrap();
        let t = open_transaction(&td);
        let scope = ChangesRef::Local {
            branch: "main".to_string(),
        };

        store_comment_node_ids(&t, "change/1", &[("hash-a".into(), "gid-a".into())], &scope)
            .unwrap();
        store_comment_node_ids(&t, "change/1", &[("hash-b".into(), "gid-b".into())], &scope)
            .unwrap();
        store_comment_node_ids(&t, "change/2", &[("hash-c".into(), "gid-c".into())], &scope)
            .unwrap();
        // Overwriting an existing entry replaces just that leaf.
        store_comment_node_ids(
            &t,
            "change/1",
            &[("hash-a".into(), "gid-a2".into())],
            &scope,
        )
        .unwrap();

        let ids = read_comment_node_ids(&t, "change/1", &scope).unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids.get("hash-a").unwrap(), "gid-a2");
        assert_eq!(ids.get("hash-b").unwrap(), "gid-b");
        let other = read_comment_node_ids(&t, "change/2", &scope).unwrap();
        assert_eq!(other.get("hash-c").unwrap(), "gid-c");

        let vote = VoteData {
            state: "approve".to_string(),
            sha: "abc".to_string(),
        };
        store_vote_node_ids(&t, "change/1", &[("alice".into(), vote.clone())], &scope).unwrap();
        store_vote_node_ids(&t, "change/1", &[("bob".into(), vote.clone())], &scope).unwrap();

        let votes = read_vote_node_ids(&t, "change/1", &scope).unwrap();
        assert_eq!(votes.len(), 2);
        // Writing votes must not disturb the comment ids.
        let ids = read_comment_node_ids(&t, "change/1", &scope).unwrap();
        assert_eq!(ids.len(), 2);
    }
}
