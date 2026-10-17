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

#[derive(Debug, serde::Serialize)]
struct PublicationData {
    pushed: String,
    landed: bool,
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

fn imported_metadata_tree(
    transaction: &Transaction,
    change_id: &str,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let mut tips = Vec::new();
    transaction.for_each_ref_prefixed("refs/josh/changes/", |_, oid| {
        tips.push(oid);
        Ok(())
    })?;

    let mut merged = crate::layout::ChangesRefData::default();
    let mut found = false;
    for tip in tips {
        let tree = objects::CommitData::read(transaction.odb(), tip)?.tree_id()?;
        let data: crate::layout::ChangesRefData = deserialize_tree(transaction, tree)?;
        if let Some(comments) = data.comments.get(change_id) {
            merged
                .comments
                .entry(change_id.to_owned())
                .or_default()
                .extend(comments.clone());
            found = true;
        }
        if let Some(votes) = data.votes.get(change_id) {
            merged
                .votes
                .entry(change_id.to_owned())
                .or_default()
                .extend(votes.clone());
            found = true;
        }
        if let Some(comments) = data.outbox.comments.get(change_id) {
            merged
                .outbox
                .comments
                .entry(change_id.to_owned())
                .or_default()
                .extend(comments.clone());
            found = true;
        }
        if let Some(votes) = data.outbox.votes.get(change_id) {
            merged
                .outbox
                .votes
                .entry(change_id.to_owned())
                .or_default()
                .extend(votes.clone());
            found = true;
        }
    }
    if !found {
        return Ok(None);
    }
    let value = josh_git_serde::to_value(&merged)?;
    Ok(Some(josh_git_serde::to_tree_oid(
        transaction.odb(),
        &value,
    )?))
}

/// Merge imported worktree comments, votes, and outbox state into canonical
/// logical changes in the synthetic superhistory.
pub fn merge_canonical_metadata(transaction: &Transaction) -> anyhow::Result<()> {
    let mut tips = Vec::new();
    transaction.for_each_ref_prefixed("refs/josh/changes/", |_, oid| {
        tips.push(oid);
        Ok(())
    })?;
    let mut change_ids = std::collections::BTreeSet::new();
    for tip in tips {
        let tree = objects::CommitData::read(transaction.odb(), tip)?.tree_id()?;
        let data: crate::layout::ChangesRefData = deserialize_tree(transaction, tree)?;
        change_ids.extend(data.comments.keys().cloned());
        change_ids.extend(data.votes.keys().cloned());
        change_ids.extend(data.outbox.comments.keys().cloned());
        change_ids.extend(data.outbox.votes.keys().cloned());
    }
    if change_ids.is_empty() {
        return Ok(());
    }

    let odb = transaction.odb();
    let super_ref = "refs/josh/super";
    let previous = transaction.resolve_ref(super_ref)?;
    let mut tree = match previous {
        Some(previous) => objects::CommitData::read(odb, previous)?.tree_id()?,
        None => tree::empty_id(),
    };
    let mut changed = false;
    for change_id in change_ids {
        let Some(metadata) = imported_metadata_tree(transaction, &change_id)? else {
            continue;
        };
        let path = std::path::Path::new("changes")
            .join(encode_change_id_path(&change_id))
            .join("metadata");
        let unchanged = tree::get_path_entry(transaction, odb, tree, &path)?
            .is_some_and(|entry| entry.mode.is_tree() && entry.oid == metadata);
        if !unchanged {
            tree = tree::insert_oid(odb, tree, &path, metadata, 0o040000)?;
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }

    let signature = josh_core::git::josh_actor_signature()?;
    let parents: Vec<_> = previous.into_iter().collect();
    let commit = objects::write_commit(
        odb,
        tree,
        &parents,
        &signature,
        &signature,
        "Update canonical josh change metadata",
    )?;
    transaction.update_ref(
        super_ref,
        previous.map_or(Expected::Absent, Expected::At),
        commit,
        "josh canonical change metadata",
    )
}

/// Persist native change revisions in the store-wide synthetic superhistory.
///
/// Canonical metadata advances `refs/josh/super` without making remote
/// history its parent. A separate per-remote-branch retention history keeps
/// every projected revision reachable through anchor parents.
pub fn store_canonical_revisions(
    transaction: &Transaction,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
    remote: &str,
    branch: &str,
) -> anyhow::Result<Vec<Change>> {
    let mut changes = crate::change::split_changes(
        transaction,
        crate::change::get_changes(transaction, tip, base)?,
    )?;
    changes.sort_by(|left, right| left.id().cmp(&right.id()));
    if changes.is_empty() {
        return Ok(changes);
    }

    let odb = transaction.odb();
    let super_ref = "refs/josh/super";
    let previous_super = transaction.resolve_ref(super_ref)?;
    let mut super_tree = match previous_super {
        Some(previous) => objects::CommitData::read(odb, previous)?.tree_id()?,
        None => tree::empty_id(),
    };
    let mut anchors = Vec::new();
    let mut super_changed = false;

    for change in &changes {
        let change_id = change
            .id()
            .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;
        let revision = DiffData {
            commit: change.commit().to_string(),
            base: change.base().to_string(),
        };
        let value = josh_git_serde::to_value(&revision)?;
        let revision_tree = josh_git_serde::to_tree_oid(odb, &value)?;
        let change_path = std::path::Path::new("changes").join(encode_change_id_path(change_id));
        let projection_path = change_path
            .join("projections")
            .join(remote)
            .join("refs")
            .join("heads")
            .join(branch);
        let publication = PublicationData {
            pushed: change.commit().to_string(),
            landed: false,
        };
        let publication_tree =
            josh_git_serde::to_tree_oid(odb, &josh_git_serde::to_value(&publication)?)?;
        let publication_path = change_path
            .join("publications")
            .join(remote)
            .join("refs")
            .join("heads")
            .join(branch);
        let metadata_tree = imported_metadata_tree(transaction, change_id)?;
        let metadata_path = change_path.join("metadata");
        let projection_unchanged =
            tree::get_path_entry(transaction, odb, super_tree, &projection_path)?
                .is_some_and(|entry| entry.mode.is_tree() && entry.oid == revision_tree);
        let publication_unchanged =
            tree::get_path_entry(transaction, odb, super_tree, &publication_path)?
                .is_some_and(|entry| entry.mode.is_tree() && entry.oid == publication_tree);
        let metadata_unchanged = match metadata_tree {
            Some(metadata_tree) => {
                tree::get_path_entry(transaction, odb, super_tree, &metadata_path)?
                    .is_some_and(|entry| entry.mode.is_tree() && entry.oid == metadata_tree)
            }
            None => true,
        };
        if projection_unchanged && publication_unchanged && metadata_unchanged {
            continue;
        }
        if !projection_unchanged {
            super_tree =
                tree::insert_oid(odb, super_tree, &projection_path, revision_tree, 0o040000)?;
            let signature = josh_core::git::josh_actor_signature()?;
            let anchor = objects::write_commit(
                odb,
                tree::empty_id(),
                &[change.commit()],
                &signature,
                &signature,
                "Retain josh change revision",
            )?;
            anchors.push(anchor);
        }
        if !publication_unchanged {
            super_tree = tree::insert_oid(
                odb,
                super_tree,
                &publication_path,
                publication_tree,
                0o040000,
            )?;
        }
        if let Some(metadata_tree) = metadata_tree
            && !metadata_unchanged
        {
            super_tree =
                tree::insert_oid(odb, super_tree, &metadata_path, metadata_tree, 0o040000)?;
        }
        super_changed = true;
    }

    if !super_changed {
        return Ok(changes);
    }

    let signature = josh_core::git::josh_actor_signature()?;
    if !anchors.is_empty() {
        let retention_ref = format!("refs/josh/remotes/{remote}/changes/{branch}");
        let previous_retention = transaction.resolve_ref(&retention_ref)?;
        let mut retention_parents: Vec<_> = previous_retention.into_iter().collect();
        retention_parents.extend(anchors);
        let retention = objects::write_commit(
            odb,
            tree::empty_id(),
            &retention_parents,
            &signature,
            &signature,
            "Retain josh change revisions",
        )?;
        transaction.update_ref(
            &retention_ref,
            previous_retention.map_or(Expected::Absent, Expected::At),
            retention,
            "josh change retention",
        )?;
    }

    let super_parents: Vec<_> = previous_super.into_iter().collect();
    let super_commit = objects::write_commit(
        odb,
        super_tree,
        &super_parents,
        &signature,
        &signature,
        "Update canonical josh changes",
    )?;
    transaction.update_ref(
        super_ref,
        previous_super.map_or(Expected::Absent, Expected::At),
        super_commit,
        "josh canonical changes",
    )?;
    Ok(changes)
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

    #[test]
    fn canonical_revisions_keep_superhistory_linear_and_retain_amends() {
        let td = tempfile::tempdir().unwrap();
        let t = open_transaction(&td);
        let null = gix_hash::ObjectId::null(gix_hash::Kind::Sha1);
        let v1 = commit_with_change_id(&td, "change/10", "v1");

        store_canonical_revisions(&t, v1, null, "origin", "main").unwrap();
        let super_v1 = t.resolve_ref("refs/josh/super").unwrap().unwrap();
        let retention_v1 = t
            .resolve_ref("refs/josh/remotes/origin/changes/main")
            .unwrap()
            .unwrap();
        let projection = std::path::Path::new("changes")
            .join("change%2F10")
            .join("projections/origin/refs/heads/main");
        let super_tree = objects::CommitData::read(t.odb(), super_v1)
            .unwrap()
            .tree_id()
            .unwrap();
        let projection_tree = tree::get_path_entry(&t, t.odb(), super_tree, &projection)
            .unwrap()
            .unwrap()
            .oid;
        let revision: DiffData = deserialize_tree(&t, projection_tree).unwrap();
        assert_eq!(revision.commit, v1.to_string());
        assert!(
            objects::CommitData::read(t.odb(), super_v1)
                .unwrap()
                .first_parent_id()
                .is_none()
        );

        let retention = objects::CommitData::read(t.odb(), retention_v1).unwrap();
        let anchor = retention.first_parent_id().unwrap();
        assert_eq!(
            objects::CommitData::read(t.odb(), anchor)
                .unwrap()
                .first_parent_id(),
            Some(v1)
        );

        let v2 = commit_with_change_id(&td, "change/10", "v2");
        store_canonical_revisions(&t, v2, null, "origin", "main").unwrap();
        let super_v2 = t.resolve_ref("refs/josh/super").unwrap().unwrap();
        let retention_v2 = t
            .resolve_ref("refs/josh/remotes/origin/changes/main")
            .unwrap()
            .unwrap();
        assert_eq!(
            objects::CommitData::read(t.odb(), super_v2)
                .unwrap()
                .first_parent_id(),
            Some(super_v1)
        );
        assert_eq!(
            objects::CommitData::read(t.odb(), retention_v2)
                .unwrap()
                .first_parent_id(),
            Some(retention_v1)
        );

        let change_v2 = Change::new(&t, v2).unwrap();
        let local_scope = ChangesRef::Local {
            branch: "main".to_owned(),
        };
        crate::comments::write_comment(
            &t,
            &change_v2,
            &crate::comments::CommentMeta {
                message: "shared".to_owned(),
                file: None,
                location: None,
                reply_to: None,
                update_of: None,
            },
            None,
            None,
            &local_scope,
            CommentNamespace::Default,
        )
        .unwrap();
        store_canonical_revisions(&t, v2, null, "origin", "main").unwrap();
        let super_with_metadata = t.resolve_ref("refs/josh/super").unwrap().unwrap();
        assert_ne!(super_with_metadata, super_v2);
        assert_eq!(
            t.resolve_ref("refs/josh/remotes/origin/changes/main")
                .unwrap(),
            Some(retention_v2)
        );
        let metadata_path = std::path::Path::new("changes/change%2F10/metadata");
        let metadata_tree = objects::CommitData::read(t.odb(), super_with_metadata)
            .unwrap()
            .tree_id()
            .unwrap();
        assert!(
            tree::get_path_entry(&t, t.odb(), metadata_tree, metadata_path)
                .unwrap()
                .is_some()
        );

        store_canonical_revisions(&t, v2, null, "origin", "main").unwrap();
        assert_eq!(
            t.resolve_ref("refs/josh/super").unwrap(),
            Some(super_with_metadata)
        );
        assert_eq!(
            t.resolve_ref("refs/josh/remotes/origin/changes/main")
                .unwrap(),
            Some(retention_v2)
        );
    }
}
