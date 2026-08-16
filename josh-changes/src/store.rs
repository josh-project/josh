use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use josh_core::cache::{Expected, Transaction};

pub(crate) fn get_tree<'a>(
    repo: &'a git2::Repository,
    tree: &'a git2::Tree,
    path: &std::path::Path,
) -> Option<git2::Tree<'a>> {
    tree.get_path(path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
        .and_then(|o| o.peel_to_tree().ok())
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
    let ref_name = scope.ref_name();
    let prev_commit = transaction
        .resolve_ref(&ref_name)?
        .and_then(|oid| repo.find_commit(oid).ok());
    let base_tree = prev_commit
        .as_ref()
        .and_then(|c| c.tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    // Skip if the blob already exists at this path.
    if let Some(existing) = base_tree
        .get_path(path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == blob_oid {
            return Ok(());
        }
    }

    let tree = josh_core::filter::tree::insert(
        repo,
        &base_tree,
        path,
        blob_oid,
        git2::FileMode::Blob.into(),
    )?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => josh_core::git::user_signature(repo)?,
    };
    let parents: Vec<&git2::Commit> = prev_commit.iter().collect();
    let msg = format!("update {}\n", ref_name);
    let new_oid = repo.commit(None, &sig, &sig, &msg, &tree, &parents)?;
    transaction.update_ref(
        &ref_name,
        prev_commit
            .as_ref()
            .map_or(Expected::Absent, |c| Expected::At(c.id())),
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
    let repo = transaction.repo();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => repo.find_commit(oid)?.tree()?,
        None => return Ok(Default::default()),
    };
    let subtree = match get_tree(repo, &tree, path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut map = std::collections::HashMap::new();
    for entry in subtree.iter() {
        if let Ok(name) = entry.name() {
            if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
                map.insert(
                    name.to_string(),
                    String::from_utf8_lossy(blob.content()).to_string(),
                );
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
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let commit_oid_str = change.commit().to_string();
    let base_str = change.base().to_string();
    let content = format!("{}\n{}", commit_oid_str, base_str);
    let blob_oid = repo.blob(content.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    let entry_name = blob_oid.to_string();
    tb.insert(&entry_name, blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let ref_name = scope.ref_name();
    let prev_tip = transaction
        .resolve_ref(&ref_name)?
        .and_then(|oid| repo.find_commit(oid).ok());
    let base_tree = prev_tip
        .as_ref()
        .and_then(|c| c.tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    let path = std::path::Path::new("diffs").join(encode_change_id_path(&change_id));

    if let Some(existing) = base_tree
        .get_path(&path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == tree_oid {
            return Ok(());
        }
    }

    let tree = josh_core::filter::tree::insert(repo, &base_tree, &path, tree_oid, 0o0040000)?;

    let sig = repo.signature()?;

    let source_commit = repo.find_commit(change.commit())?;
    let anchor_sig = git2::Signature::new("JOSH", "josh@josh-project.dev", &git2::Time::new(0, 0))?;
    let empty_tree = repo.find_tree(josh_core::filter::tree::empty_id())?;
    let anchor_oid = repo.commit(
        None,
        &anchor_sig,
        &anchor_sig,
        "josh\n",
        &empty_tree,
        &[&source_commit],
    )?;
    let anchor_commit = repo.find_commit(anchor_oid)?;

    let mut parents: Vec<&git2::Commit> = Vec::new();
    if let Some(ref c) = prev_tip {
        parents.push(c);
    }
    parents.push(&anchor_commit);
    let msg = format!("update {}\n", ref_name);
    let new_oid = repo.commit(None, &sig, &sig, &msg, &tree, &parents)?;
    transaction.update_ref(
        &ref_name,
        prev_tip
            .as_ref()
            .map_or(Expected::Absent, |c| Expected::At(c.id())),
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
    let blob_oid = repo.blob(json.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    tb.insert(&blob_oid.to_string(), blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let ref_name = scope.ref_name();
    let prev_tip = transaction
        .resolve_ref(&ref_name)?
        .and_then(|oid| repo.find_commit(oid).ok());
    let base_tree = prev_tip
        .as_ref()
        .and_then(|c| c.tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    let path = std::path::Path::new("gh").join(encode_change_id_path(change_id));

    if let Some(existing) = base_tree
        .get_path(&path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == tree_oid {
            return Ok(());
        }
    }

    let tree = josh_core::filter::tree::insert(repo, &base_tree, &path, tree_oid, 0o0040000)?;

    let sig = repo.signature()?;
    let parents: Vec<&git2::Commit> = prev_tip.iter().collect();
    let msg = format!("update {}\n", ref_name);
    let new_oid = repo.commit(None, &sig, &sig, &msg, &tree, &parents)?;
    transaction.update_ref(
        &ref_name,
        prev_tip
            .as_ref()
            .map_or(Expected::Absent, |c| Expected::At(c.id())),
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
    let repo = transaction.repo();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => repo.find_commit(oid)?.tree()?,
        None => return Ok(None),
    };
    let gh_path = std::path::Path::new("gh").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &gh_path) {
        Some(t) => t,
        None => return Ok(None),
    };
    for entry in subtree.iter() {
        if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
            return Ok(Some(String::from_utf8_lossy(blob.content()).to_string()));
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
    let encoded = encode_change_id_path(change_id);

    let ref_name = scope.ref_name();
    let prev_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => repo.find_commit(oid)?,
        None => return Ok(()),
    };
    let base_tree = prev_commit.tree()?;

    let mut tree = base_tree;
    for prefix in &[
        "diffs",
        "comments/C",
        "comments/F",
        "outbox/comments/C",
        "outbox/comments/F",
        "gh",
        "gh_ids",
        "votes",
        "outbox/votes",
        "gh_vote_ids",
    ] {
        let path = std::path::Path::new(prefix).join(&encoded);
        if tree.get_path(&path).is_ok() {
            tree = josh_core::filter::tree::insert(repo, &tree, &path, git2::Oid::ZERO_SHA1, 0)?;
        }
    }

    let sig = repo.signature()?;
    let msg = format!("update {}\n", ref_name);
    let new_oid = repo.commit(None, &sig, &sig, &msg, &tree, &[&prev_commit])?;
    transaction.update_ref(&ref_name, Expected::At(prev_commit.id()), new_oid, &msg)?;

    Ok(())
}
