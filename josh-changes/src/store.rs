use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;

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

pub(crate) fn write_changes_tree(
    repo: &git2::Repository,
    path: &std::path::Path,
    blob_oid: git2::Oid,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let ref_name = scope.ref_name();
    let base_tree = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
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
    let parent_commit = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    repo.commit(
        Some(&ref_name),
        &sig,
        &sig,
        &format!("update {}\n", ref_name),
        &tree,
        &parents,
    )?;
    Ok(())
}

pub fn store_diff_data(
    repo: &git2::Repository,
    change: &Change,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
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
    let base_tree = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
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
    let prev_tip = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());

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
    repo.commit(
        Some(&ref_name),
        &sig,
        &sig,
        &format!("update {}\n", ref_name),
        &tree,
        &parents,
    )?;

    Ok(())
}

pub fn store_pr_data(
    repo: &git2::Repository,
    change_id: &str,
    json: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let blob_oid = repo.blob(json.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    tb.insert(&blob_oid.to_string(), blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let ref_name = scope.ref_name();
    let base_tree = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
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
    let prev_tip = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = prev_tip.iter().collect();
    repo.commit(
        Some(&ref_name),
        &sig,
        &sig,
        &format!("update {}\n", ref_name),
        &tree,
        &parents,
    )?;

    Ok(())
}

/// Read stored GitHub PR data JSON for a change, if it exists.
pub fn read_pr_data(
    repo: &git2::Repository,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Option<String>> {
    let tree = match repo.find_reference(&scope.ref_name()) {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(None),
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
    repo: &git2::Repository,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let encoded = encode_change_id_path(change_id);

    let ref_name = scope.ref_name();
    let base_tree = match repo.find_reference(&ref_name) {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(()),
    };

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
    let parent_commit = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    repo.commit(
        Some(&ref_name),
        &sig,
        &sig,
        &format!("update {}\n", ref_name),
        &tree,
        &parents,
    )?;

    Ok(())
}
