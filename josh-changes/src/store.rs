use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::memodb::Odb;
use josh_core::objects;
use josh_git_serde::GitValue;

/// The subtree at `path`, or `None` when it is missing or not a tree.
pub(crate) fn get_tree(
    transaction: &Transaction,
    odb: &Odb,
    root: git2::Oid,
    path: &std::path::Path,
) -> Option<git2::Oid> {
    tree::get_path_entry(transaction, odb, root, path)
        .ok()
        .flatten()
        .filter(|entry| entry.mode.is_tree())
        .map(|entry| objects::git2_oid(&entry.oid))
}

/// The tree of `scope`'s ref, or `None` when the ref does not exist.
pub fn scope_tree(
    transaction: &Transaction,
    odb: &Odb,
    scope: &ChangesRef,
) -> anyhow::Result<Option<git2::Oid>> {
    match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => Ok(Some(objects::CommitData::read(odb, oid)?.tree_id()?)),
        None => Ok(None),
    }
}

pub(crate) fn parse_timestamp(s: Option<&str>) -> git2::Time {
    let Some(s) = s else {
        return git2::Time::new(0, 0);
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) else {
        return git2::Time::new(0, 0);
    };
    git2::Time::new(dt.timestamp(), dt.offset().local_minus_utc() / 60)
}

/// Write `blob_oid` at `path` inside `scope`'s ref, committing the updated
/// tree onto the ref. No-op when the same blob is already present at `path`.
///
/// This is the generic write primitive for path-keyed metadata on changes
/// refs; forge crates build their on-ref layouts on top of it.
pub fn write_changes_tree(
    transaction: &Transaction,
    path: &std::path::Path,
    blob_oid: git2::Oid,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let ref_name = scope.ref_name();
    let prev_commit = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_commit {
        Some(oid) => objects::CommitData::read(odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    // Skip if the blob already exists at this path.
    if let Ok(Some(existing)) = tree::get_path_entry(transaction, odb, base_tree, path) {
        if objects::git2_oid(&existing.oid) == blob_oid {
            return Ok(());
        }
    }

    let tree = tree::insert_oid(odb, base_tree, path, blob_oid, git2::FileMode::Blob.into())?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => josh_core::git::user_signature(transaction)?,
    };
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(odb, tree, prev_commit.as_slice(), &sig, &sig, &msg)?;
    transaction.update_ref(
        &ref_name,
        prev_commit.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;
    Ok(())
}

/// Write `value` at `path` inside `scope`'s ref as git objects -- structs and
/// maps become trees, scalars blobs -- committing the updated tree onto the
/// ref. Returns the root object id: the value's canonical identity, usable as
/// a dedup key. No-op (returning the same id) when an identical value already
/// sits at `path`.
pub fn write_value<T: serde::Serialize>(
    transaction: &Transaction,
    path: &std::path::Path,
    data: &T,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<git2::Oid> {
    let (root, mode) = value_oid(transaction, data)?;
    place_oid(transaction, path, root, mode, author, timestamp, scope)?;
    Ok(root)
}

/// Serialize `data` into git objects without placing them at any path,
/// returning the root object id and its tree-entry mode. For content-addressed
/// layouts where the id is a path component (e.g. comments); pair with
/// [`place_oid`].
pub fn value_oid<T: serde::Serialize>(
    transaction: &Transaction,
    data: &T,
) -> anyhow::Result<(git2::Oid, i32)> {
    let odb = transaction.odb();
    let value = josh_git_serde::to_value(data)?;
    let root = josh_git_serde::to_tree_oid(odb, &value)?;
    let mode: i32 = match &value {
        GitValue::Tree(_) => 0o0040000,
        GitValue::Blob(_) => git2::FileMode::Blob.into(),
    };
    Ok((objects::git2_oid(&root), mode))
}

/// Place the already-written object `root` at `path` inside `scope`'s ref and
/// commit the updated tree. No-op when `root` already sits at `path`.
pub fn place_oid(
    transaction: &Transaction,
    path: &std::path::Path,
    root: git2::Oid,
    mode: i32,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let ref_name = scope.ref_name();
    let prev_commit = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_commit {
        Some(oid) => objects::CommitData::read(odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    if let Ok(Some(existing)) = tree::get_path_entry(transaction, odb, base_tree, path) {
        if objects::git2_oid(&existing.oid) == root {
            return Ok(());
        }
    }

    let tree = tree::insert_oid(odb, base_tree, path, root, mode)?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => josh_core::git::user_signature(transaction)?,
    };
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(&odb, tree, prev_commit.as_slice(), &sig, &sig, &msg)?;
    transaction.update_ref(
        &ref_name,
        prev_commit.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;
    Ok(())
}

/// Select `dir` in place: the filtered tree keeps the entry under its name,
/// so the matching field of a layout struct populates and the rest default.
pub fn namespace_filter(dir: &str) -> josh_core::filter::Filter {
    josh_core::filter::Filter::new().subdir(dir).prefix(dir)
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
    let value = josh_git_serde::from_tree_oid(odb, objects::gix_oid(filtered.tree_id()))?;
    Ok(Some(josh_git_serde::from_value(&value)?))
}

/// The change's tip and base commits, stored as a tree at `diffs/<change-id>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffData {
    pub commit: String,
    pub base: String,
}

pub fn store_diff_data(
    transaction: &Transaction,
    change: &Change,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let data = DiffData {
        commit: change.commit().to_string(),
        base: change.base().to_string(),
    };
    let value = josh_git_serde::to_value(&data)?;
    let tree_oid = josh_git_serde::to_tree_oid(odb, &value)?;

    let ref_name = scope.ref_name();
    let prev_tip = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_tip {
        Some(oid) => objects::CommitData::read(odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    let path = std::path::Path::new("diffs").join(encode_change_id_path(&change_id));

    if let Ok(Some(existing)) = tree::get_path_entry(transaction, odb, base_tree, &path) {
        if existing.oid == tree_oid {
            return Ok(());
        }
    }

    let tree = tree::insert_oid(
        odb,
        base_tree,
        &path,
        objects::git2_oid(&tree_oid),
        0o0040000,
    )?;

    let sig = transaction.signature()?;

    let anchor_sig = git2::Signature::new("JOSH", "josh@josh-project.dev", &git2::Time::new(0, 0))?;
    let anchor_oid = objects::write_commit(
        odb,
        tree::empty_id(),
        &[change.commit()],
        &anchor_sig,
        &anchor_sig,
        "josh\n",
    )?;

    let mut parents: Vec<git2::Oid> = Vec::new();
    parents.extend(prev_tip);
    parents.push(anchor_oid);
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(odb, tree, &parents, &sig, &sig, &msg)?;
    transaction.update_ref(
        &ref_name,
        prev_tip.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;

    Ok(())
}

pub fn store_pr_data<T: serde::Serialize>(
    transaction: &Transaction,
    change_id: &str,
    data: &T,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let path = std::path::Path::new("gh").join(encode_change_id_path(change_id));
    write_value(transaction, &path, data, None, None, scope)?;
    Ok(())
}

/// Delete all stored data for a change from the given changes ref.
/// Removes entries from diffs/, comments/{C,F}/, outbox/comments/{C,F}/,
/// gh/, and gh_ids/ subtrees.
pub fn delete_change(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let encoded = encode_change_id_path(change_id);

    let ref_name = scope.ref_name();
    let prev_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => oid,
        None => return Ok(()),
    };
    let mut tree = objects::CommitData::read(odb, prev_commit)?.tree_id()?;
    for prefix in &[
        "diffs",
        "comments/C",
        "comments/F",
        "outbox/comments/C",
        "outbox/comments/F",
        "gh",
        "gh_ids",
        "gh_vote_ids",
        "gh_cache",
        "votes",
        "outbox/votes",
    ] {
        let path = std::path::Path::new(prefix).join(&encoded);
        if matches!(
            tree::get_path_entry(transaction, odb, tree, &path),
            Ok(Some(_))
        ) {
            tree = tree::insert_oid(odb, tree, &path, git2::Oid::ZERO_SHA1, 0)?;
        }
    }

    let sig = transaction.signature()?;
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(odb, tree, &[prev_commit], &sig, &sig, &msg)?;
    transaction.update_ref(&ref_name, Expected::At(prev_commit), new_oid, &msg)?;

    Ok(())
}
