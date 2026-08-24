use crate::change::{Change, encode_change_id_path};
use crate::comments::CommentNamespace;
use crate::layout::DIFFS_PATH;
use crate::refs::ChangesRef;
use crate::votes::VoteNamespace;
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::memodb::Odb;
use josh_core::objects;
use josh_git_serde::GitValue;

/// The subtree at `path`, or `None` when it is missing or not a tree.
pub(crate) fn get_tree(
    transaction: &Transaction,
    odb: &Odb,
    root: gix_hash::ObjectId,
    path: &std::path::Path,
) -> Option<gix_hash::ObjectId> {
    tree::get_path_entry(transaction, odb, root, path)
        .ok()
        .flatten()
        .filter(|entry| entry.mode.is_tree())
        .map(|entry| entry.oid.to_owned())
}

/// The tree of `scope`'s ref, or `None` when the ref does not exist.
pub fn scope_tree(
    transaction: &Transaction,
    odb: &Odb,
    scope: &ChangesRef,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => Ok(Some(objects::CommitData::read(odb, oid)?.tree_id()?)),
        None => Ok(None),
    }
}

/// One commit of a changes ref's history: the commit plus its tree and
/// first-parent tree, resolved.
pub(crate) struct RefHistoryEntry {
    pub commit: objects::CommitData,
    pub tree: gix_hash::ObjectId,
    pub parent_tree: Option<gix_hash::ObjectId>,
}

/// First-parent walk of `head`'s history, newest first. Commits with
/// unreadable data or trees are skipped.
pub(crate) fn walk_ref_history(
    transaction: &Transaction,
    head: gix_hash::ObjectId,
) -> anyhow::Result<Vec<RefHistoryEntry>> {
    let odb = transaction.odb();
    let mut walk = objects::RevWalk::new(odb);
    walk.simplify_first_parent();
    walk.push(head)?;
    let mut out = Vec::new();
    for oid in walk.into_topo_vec(|_| false)? {
        let Ok(commit) = objects::CommitData::read(odb, oid) else {
            continue;
        };
        let Ok(tree) = commit.tree_id() else {
            continue;
        };
        let parent_tree = commit
            .first_parent_id()
            .and_then(|p| josh_core::git::read_tree_id(odb, p).ok());
        out.push(RefHistoryEntry {
            commit,
            tree,
            parent_tree,
        });
    }
    Ok(out)
}

/// Resolve `scope`'s ref to its tip commit and tree -- the empty tree when
/// the ref does not exist.
pub(crate) fn scope_base(
    transaction: &Transaction,
    scope: &ChangesRef,
) -> anyhow::Result<(Option<gix_hash::ObjectId>, gix_hash::ObjectId)> {
    let prev_commit = transaction.resolve_ref(&scope.ref_name())?;
    let tree = match prev_commit {
        Some(oid) => objects::CommitData::read(transaction.odb(), oid)?.tree_id()?,
        None => tree::empty_id(),
    };
    Ok((prev_commit, tree))
}

pub(crate) fn parse_timestamp(s: Option<&str>) -> gix_actor::date::Time {
    let Some(s) = s else {
        return gix_actor::date::Time {
            seconds: 0,
            offset: 0,
        };
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) else {
        return gix_actor::date::Time {
            seconds: 0,
            offset: 0,
        };
    };
    gix_actor::date::Time {
        seconds: dt.timestamp(),
        offset: dt.offset().local_minus_utc(),
    }
}

/// Serialize `data` into git objects and merge the value tree into `scope`'s
/// ref, narrowed by `filter` -- the inverse of `read_filtered`: the value
/// tree is embedded into full-ref shape with the filter's inverse and
/// overlaid onto the ref tree, committing the result onto the ref. The
/// overlay is a recursive merge where the value's entries win, so a sparsely
/// populated struct adds or replaces only the entries it carries and
/// everything else survives. Deletion is `delete_filtered`; a value that is the
/// complete content of the filter's scope simply carries every entry.
/// No-op when nothing changes.
pub fn write_filtered<T: serde::Serialize>(
    transaction: &Transaction,
    scope: &ChangesRef,
    filter: josh_core::filter::Filter,
    data: &T,
    author: Option<&str>,
    timestamp: Option<&str>,
) -> anyhow::Result<()> {
    let Some((prev_commit, merged)) = merge_into_ref(transaction, scope, filter, data)? else {
        return Ok(());
    };
    commit_tree(
        transaction,
        &scope.ref_name(),
        prev_commit,
        merged,
        author,
        timestamp,
        &[],
    )
}

/// Serialize `data` into git objects and merge the value tree into `scope`'s
/// ref tree, narrowed by `filter` -- the `write_filtered` front half, without
/// committing. Returns the ref's tip commit and the merged tree, or `None`
/// when nothing changes.
fn merge_into_ref<T: serde::Serialize>(
    transaction: &Transaction,
    scope: &ChangesRef,
    filter: josh_core::filter::Filter,
    data: &T,
) -> anyhow::Result<Option<(Option<gix_hash::ObjectId>, gix_hash::ObjectId)>> {
    let value = josh_git_serde::to_value(data)?;
    anyhow::ensure!(
        matches!(value, GitValue::Tree(_)),
        "changes-ref writes require a tree-shaped value"
    );
    let root = josh_git_serde::to_tree_oid(transaction.odb(), &value)?;

    let (prev_commit, base_tree) = scope_base(transaction, scope)?;

    let merged = merge_value_tree(transaction, filter, root, base_tree)?;
    if merged == base_tree {
        return Ok(None);
    }
    Ok(Some((prev_commit, merged)))
}

/// Embed the value tree `root` into full-ref shape with `filter`'s inverse
/// and overlay it onto `base_tree` -- the tail of `filter::unapply` minus
/// its `subtract` step: the filter's location gains `root`'s entries,
/// recursively merged so everything else survives.
fn merge_value_tree(
    transaction: &Transaction,
    filter: josh_core::filter::Filter,
    root: gix_hash::ObjectId,
    base_tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let embedded = josh_core::filter::apply(
        transaction,
        josh_core::filter::invert(filter)?,
        josh_core::filter::Rewrite::from_tree(root),
    )?;
    tree::overlay(transaction, embedded.tree_id(), base_tree)
}

/// Commit `tree` onto `ref_name` (creating the ref when absent), signed by
/// `author`/`timestamp` or the transaction's user. The commit's parents are
/// `prev_commit` (when present) followed by `extra_parents`.
pub(crate) fn commit_tree(
    transaction: &Transaction,
    ref_name: &str,
    prev_commit: Option<gix_hash::ObjectId>,
    tree: gix_hash::ObjectId,
    author: Option<&str>,
    timestamp: Option<&str>,
    extra_parents: &[gix_hash::ObjectId],
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let sig = match author {
        Some(name) => gix_actor::Signature {
            name: name.into(),
            email: format!("{}@github", name).into(),
            time: parse_timestamp(timestamp),
        },
        None => josh_core::git::user_signature(transaction)?,
    };
    let msg = format!("update {}\n", ref_name);
    let parents: Vec<gix_hash::ObjectId> = prev_commit
        .into_iter()
        .chain(extra_parents.iter().copied())
        .collect();
    let new_oid = objects::write_commit(odb, tree, &parents, &sig, &sig, &msg)?;
    transaction.update_ref(
        ref_name,
        prev_commit.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;
    Ok(())
}

/// Remove `paths` from `scope`'s ref in a single commit by applying one
/// `:exclude[::path]` chain to the ref tree -- the delete half of the
/// `read_filtered`/`write_filtered` family: subtraction where writes
/// overlay. Excluding a missing path is a no-op; when the tree does not
/// change, no commit is made.
pub fn delete_filtered(
    transaction: &Transaction,
    paths: &[std::path::PathBuf],
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let ref_name = scope.ref_name();
    let (prev_commit, base_tree) = scope_base(transaction, scope)?;

    let mut filter = josh_core::filter::Filter::new();
    for path in paths {
        filter = filter.exclude(josh_core::filter::Filter::new().file(path_str(path)?));
    }

    let tree = josh_core::filter::apply(
        transaction,
        filter,
        josh_core::filter::Rewrite::from_tree(base_tree),
    )?
    .tree_id();
    if tree == base_tree {
        return Ok(());
    }
    commit_tree(transaction, &ref_name, prev_commit, tree, None, None, &[])
}

pub(crate) fn path_str(path: &std::path::Path) -> anyhow::Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 path {}", path.display()))
}

/// Serialize `data` into git objects without placing them at any path,
/// returning the root object id. For content-addressed layouts where the id
/// is a path component (e.g. comments, which place the result with their own
/// private helper).
pub fn value_oid<T: serde::Serialize>(
    transaction: &Transaction,
    data: &T,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    let value = josh_git_serde::to_value(data)?;
    josh_git_serde::to_tree_oid(odb, &value)
}

/// Deserialize the tree `root` into `T`.
pub(crate) fn deserialize_tree<T: serde::de::DeserializeOwned>(
    transaction: &Transaction,
    root: gix_hash::ObjectId,
) -> anyhow::Result<T> {
    let value = josh_git_serde::from_tree_oid(transaction.odb(), root)?;
    Ok(josh_git_serde::from_value(&value)?)
}

/// Select `path` in place: the filtered tree keeps the entry under its name,
/// so the matching field of a layout struct populates and the rest default.
/// The `subdir(path).prefix(path)` chain is self-inverse, so the same filter
/// embeds a serialized value tree back into full-ref shape on write
/// (`write_filtered`).
pub fn namespace_filter(path: &str) -> josh_core::filter::Filter {
    josh_core::filter::Filter::new().subdir(path).prefix(path)
}

/// Deserialize `scope`'s ref tree, narrowed by `filter`, into `T`. `None`
/// when the ref does not exist. Entries the filter does not select are
/// absent from the filtered tree, so `T` should tolerate missing fields
/// (`#[serde(default)]`).
pub fn read_filtered<T: serde::de::DeserializeOwned>(
    transaction: &Transaction,
    scope: &ChangesRef,
    filter: josh_core::filter::Filter,
) -> anyhow::Result<Option<T>> {
    let odb = transaction.odb();
    let Some(root) = scope_tree(transaction, odb, scope)? else {
        return Ok(None);
    };
    let filtered = josh_core::filter::apply(
        transaction,
        filter,
        josh_core::filter::Rewrite::from_tree(root),
    )?;
    Ok(Some(deserialize_tree(transaction, filtered.tree_id())?))
}

/// The change's tip and base commits, stored as a tree at `diffs/<change-id>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffData {
    pub commit: String,
    pub base: String,
}

/// Merge the change's `diffs/` entry into `scope`'s ref. The commit carries
/// an anchor parent on the change's tip commit so the ref's history keeps the
/// change reachable.
pub fn store_diff_data(
    transaction: &Transaction,
    change: &Change,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let mut data = crate::layout::ChangesRefData::default();
    data.diffs.insert(
        change_id.to_string(),
        DiffData {
            commit: change.commit().to_string(),
            base: change.base().to_string(),
        },
    );
    let Some((prev_tip, tree)) =
        merge_into_ref(transaction, scope, namespace_filter(DIFFS_PATH), &data)?
    else {
        return Ok(());
    };

    let anchor_sig = gix_actor::Signature {
        name: "JOSH".into(),
        email: "josh@josh-project.dev".into(),
        time: gix_actor::date::Time {
            seconds: 0,
            offset: 0,
        },
    };
    let anchor_oid = objects::write_commit(
        transaction.odb(),
        tree::empty_id(),
        &[change.commit()],
        &anchor_sig,
        &anchor_sig,
        "josh\n",
    )?;

    commit_tree(
        transaction,
        &scope.ref_name(),
        prev_tip,
        tree,
        None,
        None,
        &[anchor_oid],
    )
}

/// Delete all stored data for a change from the given changes ref: the
/// change's entry under every core namespace (diffs, votes, comments, and
/// their outbox counterparts) plus any caller-supplied forge namespaces in
/// `extra_namespaces`.
pub fn delete_change(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    extra_namespaces: &[&str],
) -> anyhow::Result<()> {
    let encoded = encode_change_id_path(change_id);
    let core = [
        DIFFS_PATH,
        CommentNamespace::Default.path(),
        CommentNamespace::Outbox.path(),
        VoteNamespace::Default.path(),
        VoteNamespace::Outbox.path(),
    ];
    let paths: Vec<std::path::PathBuf> = core
        .into_iter()
        .chain(extra_namespaces.iter().copied())
        .map(|prefix| std::path::Path::new(prefix).join(&encoded))
        .collect();
    delete_filtered(transaction, &paths, scope)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Change;
    use crate::layout::ChangesRefData;
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

    fn commit_with_change_id(
        td: &tempfile::TempDir,
        change_id: &str,
        subject: &str,
    ) -> gix_hash::ObjectId {
        let repo = gix::open(td.path()).unwrap();
        let sig = gix_actor::Signature {
            name: "test".into(),
            email: "test@example.com".into(),
            time: gix_actor::date::Time {
                seconds: 0,
                offset: 0,
            },
        };
        josh_gix_ext::write_commit(
            &repo.objects,
            tree::empty_id(),
            &[],
            &sig,
            &sig,
            &format!("{}\n\nChange-Id: {}\n", subject, change_id),
        )
        .unwrap()
    }

    #[test]
    fn diff_and_vote_writes_merge_and_roundtrip() {
        let td = tempfile::tempdir().unwrap();
        let t = open_transaction(&td);
        let scope = ChangesRef::Local {
            branch: "main".to_string(),
        };
        let change = Change::new(&t, commit_with_change_id(&td, "change/1", "subject")).unwrap();

        // Diff write: lands, is idempotent, and carries the anchor parent.
        store_diff_data(&t, &change, &scope).unwrap();
        let tip = t.resolve_ref(&scope.ref_name()).unwrap().unwrap();
        store_diff_data(&t, &change, &scope).unwrap();
        assert_eq!(Some(tip), t.resolve_ref(&scope.ref_name()).unwrap());
        let tip_data = objects::CommitData::read(t.odb(), tip).unwrap();
        let tip_parsed = tip_data.parsed().unwrap();
        let tip_parents: Vec<_> = tip_parsed.parents().collect();
        assert_eq!(tip_parents.len(), 1);
        // The anchor commit pins the change's tip commit in the ref history.
        let anchor = objects::CommitData::read(t.odb(), tip_parents[0]).unwrap();
        let anchor_parsed = anchor.parsed().unwrap();
        let anchor_parents: Vec<_> = anchor_parsed.parents().collect();
        assert_eq!(anchor_parents.len(), 1);
        assert_eq!(anchor_parents[0].to_string(), change.commit().to_string());

        // Vote writes: merge across users, replace per user.
        let default = VoteNamespace::Default;
        crate::votes::write_vote(&t, &change, "approve", Some("alice"), None, &scope, default)
            .unwrap();
        crate::votes::write_vote(&t, &change, "reject", Some("bob"), None, &scope, default)
            .unwrap();
        crate::votes::write_vote(&t, &change, "approve", Some("alice"), None, &scope, default)
            .unwrap();

        let votes = read_filtered::<ChangesRefData>(&t, &scope, namespace_filter(default.path()))
            .unwrap()
            .unwrap();
        let per_change = votes.votes.get("change/1").unwrap();
        assert_eq!(per_change.len(), 2);
        assert_eq!(per_change.get("alice").unwrap().state, "approve");
        assert_eq!(per_change.get("bob").unwrap().state, "reject");

        // The diff entry survives the vote writes.
        let diffs = read_filtered::<ChangesRefData>(&t, &scope, namespace_filter(DIFFS_PATH))
            .unwrap()
            .unwrap();
        assert!(diffs.diffs.contains_key("change/1"));

        // Outbox votes land in their own namespace and delete cleanly.
        let remote = ChangesRef::Remote {
            remote: "origin".to_string(),
            branch: "main".to_string(),
        };
        crate::votes::write_vote(
            &t,
            &change,
            "approve",
            Some("alice"),
            None,
            &remote,
            VoteNamespace::Outbox,
        )
        .unwrap();
        crate::votes::delete_outbox_votes(
            &t,
            change.id().unwrap(),
            &remote,
            &["alice".to_string()],
        )
        .unwrap();
        let outbox = read_filtered::<ChangesRefData>(
            &t,
            &remote,
            namespace_filter(VoteNamespace::Outbox.path()),
        )
        .unwrap()
        .unwrap();
        assert!(outbox.outbox.votes.is_empty());
    }

    /// Each distinct diff write is a revision; re-writing the same diff adds
    /// none.
    #[test]
    fn revisions_track_diff_updates() {
        let td = tempfile::tempdir().unwrap();
        let t = open_transaction(&td);
        let scope = ChangesRef::Local {
            branch: "main".to_string(),
        };

        let change_v1 = Change::new(&t, commit_with_change_id(&td, "change/9", "v1")).unwrap();
        store_diff_data(&t, &change_v1, &scope).unwrap();
        let change_v2 = Change::new(&t, commit_with_change_id(&td, "change/9", "v2")).unwrap();
        store_diff_data(&t, &change_v2, &scope).unwrap();
        store_diff_data(&t, &change_v2, &scope).unwrap();

        let revs = crate::revisions::read_revisions(&t, &change_v2, &scope).unwrap();
        assert_eq!(revs.len(), 2);
        // Oldest first, carrying the actual change commits.
        assert_eq!(revs[0].diff.commit, change_v1.commit().to_string());
        assert_eq!(revs[1].diff.commit, change_v2.commit().to_string());
    }
}
