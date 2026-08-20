use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::memodb::Odb;
use josh_core::objects;

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
fn scope_tree(
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
    let repo = transaction.repo();
    let odb = transaction.odb()?;
    let ref_name = scope.ref_name();
    let prev_commit = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_commit {
        Some(oid) => objects::CommitData::read(&odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    // Skip if the blob already exists at this path.
    if let Ok(Some(existing)) = tree::get_path_entry(transaction, &odb, base_tree, path) {
        if objects::git2_oid(&existing.oid) == blob_oid {
            return Ok(());
        }
    }

    let tree = tree::insert_oid(&odb, base_tree, path, blob_oid, git2::FileMode::Blob.into())?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => josh_core::git::user_signature(repo)?,
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

/// Read a flat subtree of `scope`'s ref at `path` as a map of entry name to
/// blob contents (UTF-8, lossy). Entries that are not blobs are skipped.
/// Returns an empty map when the ref or the subtree does not exist.
pub fn read_blob_map(
    transaction: &Transaction,
    scope: &ChangesRef,
    path: &std::path::Path,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let odb = transaction.odb()?;
    let tree = match scope_tree(transaction, &odb, scope)? {
        Some(tree) => tree,
        None => return Ok(Default::default()),
    };
    let subtree = match get_tree(transaction, &odb, tree, path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut map = std::collections::HashMap::new();
    for entry in tree::read_tree(transaction, &odb, subtree)?.entries() {
        if let Ok(name) = std::str::from_utf8(entry.filename) {
            if let Some(blob) = tree::blob_bytes(&odb, objects::git2_oid(&entry.oid)) {
                map.insert(name.to_string(), String::from_utf8_lossy(&blob).to_string());
            }
        }
    }
    Ok(map)
}

pub fn store_diff_data(
    transaction: &Transaction,
    change: &Change,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let odb = transaction.odb()?;
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let commit_oid_str = change.commit().to_string();
    let base_str = change.base().to_string();
    let content = format!("{}\n{}", commit_oid_str, base_str);
    let blob_oid = objects::write_blob(&odb, content.as_bytes())?;

    let entry_name = blob_oid.to_string();
    let tree_oid = tree::insert_oid(
        &odb,
        tree::empty_id(),
        std::path::Path::new(&entry_name),
        blob_oid,
        git2::FileMode::Blob.into(),
    )?;

    let ref_name = scope.ref_name();
    let prev_tip = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_tip {
        Some(oid) => objects::CommitData::read(&odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    let path = std::path::Path::new("diffs").join(encode_change_id_path(&change_id));

    if let Ok(Some(existing)) = tree::get_path_entry(transaction, &odb, base_tree, &path) {
        if objects::git2_oid(&existing.oid) == tree_oid {
            return Ok(());
        }
    }

    let tree = tree::insert_oid(&odb, base_tree, &path, tree_oid, 0o0040000)?;

    let sig = repo.signature()?;

    let anchor_sig = git2::Signature::new("JOSH", "josh@josh-project.dev", &git2::Time::new(0, 0))?;
    let anchor_oid = objects::write_commit(
        &odb,
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
    let new_oid = objects::write_commit(&odb, tree, &parents, &sig, &sig, &msg)?;
    transaction.update_ref(
        &ref_name,
        prev_tip.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;

    Ok(())
}

pub fn store_pr_data(
    transaction: &Transaction,
    change_id: &str,
    json: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let odb = transaction.odb()?;
    let blob_oid = objects::write_blob(&odb, json.as_bytes())?;

    let tree_oid = tree::insert_oid(
        &odb,
        tree::empty_id(),
        std::path::Path::new(&blob_oid.to_string()),
        blob_oid,
        git2::FileMode::Blob.into(),
    )?;

    let ref_name = scope.ref_name();
    let prev_tip = transaction.resolve_ref(&ref_name)?;
    let base_tree = match prev_tip {
        Some(oid) => objects::CommitData::read(&odb, oid)?.tree_id()?,
        None => tree::empty_id(),
    };

    let path = std::path::Path::new("gh").join(encode_change_id_path(change_id));

    if let Ok(Some(existing)) = tree::get_path_entry(transaction, &odb, base_tree, &path) {
        if objects::git2_oid(&existing.oid) == tree_oid {
            return Ok(());
        }
    }

    let tree = tree::insert_oid(&odb, base_tree, &path, tree_oid, 0o0040000)?;

    let sig = repo.signature()?;
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(&odb, tree, prev_tip.as_slice(), &sig, &sig, &msg)?;
    transaction.update_ref(
        &ref_name,
        prev_tip.map_or(Expected::Absent, Expected::At),
        new_oid,
        &msg,
    )?;

    Ok(())
}

/// Read stored GitHub PR data JSON for a change, if it exists.
pub fn read_pr_data(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Option<String>> {
    let odb = transaction.odb()?;
    let tree = match scope_tree(transaction, &odb, scope)? {
        Some(tree) => tree,
        None => return Ok(None),
    };
    let gh_path = std::path::Path::new("gh").join(encode_change_id_path(change_id));
    let subtree = match get_tree(transaction, &odb, tree, &gh_path) {
        Some(t) => t,
        None => return Ok(None),
    };
    for entry in tree::read_tree(transaction, &odb, subtree)?.entries() {
        if let Some(blob) = tree::blob_bytes(&odb, objects::git2_oid(&entry.oid)) {
            return Ok(Some(String::from_utf8_lossy(&blob).to_string()));
        }
    }
    Ok(None)
}

/// Delete all stored data for a change from the given changes ref.
/// Removes entries from diffs/, comments/{C,F}/, outbox/comments/{C,F}/,
/// gh/, and gh_ids/ subtrees.
pub fn delete_change(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let odb = transaction.odb()?;
    let encoded = encode_change_id_path(change_id);

    let ref_name = scope.ref_name();
    let prev_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => oid,
        None => return Ok(()),
    };
    let mut tree = objects::CommitData::read(&odb, prev_commit)?.tree_id()?;
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
            tree::get_path_entry(transaction, &odb, tree, &path),
            Ok(Some(_))
        ) {
            tree = tree::insert_oid(&odb, tree, &path, git2::Oid::ZERO_SHA1, 0)?;
        }
    }

    let sig = repo.signature()?;
    let msg = format!("update {}\n", ref_name);
    let new_oid = objects::write_commit(&odb, tree, &[prev_commit], &sig, &sig, &msg)?;
    transaction.update_ref(&ref_name, Expected::At(prev_commit), new_oid, &msg)?;

    Ok(())
}
