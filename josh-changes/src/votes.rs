use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use crate::store::{get_tree, parse_timestamp};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteData {
    pub state: String,
    pub sha: String,
}

pub fn write_vote(
    repo: &git2::Repository,
    change: &Change,
    state: &str,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<String> {
    write_vote_inner(repo, change, state, author, timestamp, scope, "votes")
}

/// Write a vote into the outbox subtree of a `Remote` ref. The vote is queued
/// for the next `sync --push` to post as a PR review, after which the
/// `gh_vote_ids` mapping records the post and the outbox entry can be
/// cleaned up.
pub fn write_outbox_vote(
    repo: &git2::Repository,
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
        repo,
        change,
        state,
        author,
        timestamp,
        scope,
        "outbox/votes",
    )
}

fn write_vote_inner(
    repo: &git2::Repository,
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
    let blob_oid = repo.blob(content.as_bytes())?;

    let mut tb = repo.treebuilder(None)?;
    tb.insert(&blob_oid.to_string(), blob_oid, git2::FileMode::Blob.into())?;
    let tree_oid = tb.write()?;

    let user = match author {
        Some(name) => name.to_string(),
        None => repo.signature()?.email().unwrap_or("unknown").to_string(),
    };

    let path = std::path::Path::new(path_prefix)
        .join(encode_change_id_path(&change_id))
        .join(&user);

    let ref_name = scope.ref_name();
    let base_tree = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_tree().ok())
        .unwrap_or_else(|| repo.find_tree(josh_core::filter::tree::empty_id()).unwrap());

    if let Some(existing) = base_tree
        .get_path(&path)
        .ok()
        .and_then(|e| e.to_object(repo).ok())
    {
        if existing.id() == tree_oid {
            return Ok(content_hash);
        }
    }

    let tree = josh_core::filter::tree::insert(repo, &base_tree, &path, tree_oid, 0o0040000)?;

    let sig = match author {
        Some(name) => {
            let email = format!("{}@github", name);
            let time = parse_timestamp(timestamp);
            git2::Signature::new(name, &email, &time)?
        }
        None => repo.signature()?,
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

    Ok(content_hash)
}

pub fn read_vote(
    repo: &git2::Repository,
    change_id: &str,
    user: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<Option<VoteData>> {
    let tree = match repo.find_reference(&scope.ref_name()) {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(None),
    };

    let user = match user {
        Some(name) => name.to_string(),
        None => repo.signature()?.email().unwrap_or("unknown").to_string(),
    };

    let path = std::path::Path::new("votes")
        .join(encode_change_id_path(change_id))
        .join(&user);

    let subtree = match get_tree(repo, &tree, &path) {
        Some(t) => t,
        None => return Ok(None),
    };
    for entry in subtree.iter() {
        if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
            let data: VoteData = serde_json::from_slice(blob.content())?;
            return Ok(Some(data));
        }
    }
    Ok(None)
}

pub fn list_votes(
    repo: &git2::Repository,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    list_votes_at_prefix(repo, change_id, scope, "votes")
}

/// List votes queued in the outbox subtree of `scope` (must be Remote in
/// practice; this just returns empty for refs that lack `outbox/votes`).
pub fn list_outbox_votes(
    repo: &git2::Repository,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    list_votes_at_prefix(repo, change_id, scope, "outbox/votes")
}

fn list_votes_at_prefix(
    repo: &git2::Repository,
    change_id: &str,
    scope: &ChangesRef,
    path_prefix: &str,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    let tree = match repo.find_reference(&scope.ref_name()) {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(Default::default()),
    };
    let path = std::path::Path::new(path_prefix).join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut votes = Vec::new();
    for entry in subtree.iter() {
        let user = match entry.name() {
            Ok(name) => name.to_string(),
            Err(_) => continue,
        };
        let user_tree = match entry.to_object(repo).and_then(|o| o.peel_to_tree()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for child in user_tree.iter() {
            if let Ok(blob) = child.to_object(repo).and_then(|o| o.peel_to_blob()) {
                if let Ok(data) = serde_json::from_slice::<VoteData>(blob.content()) {
                    votes.push((user.clone(), data));
                }
            }
        }
    }
    Ok(votes)
}

/// Remove outbox vote entries whose `(state, sha)` has already been recorded
/// in the `gh_vote_ids` map for the change. Called by `post_local_votes` after
/// a successful push so the outbox doesn't accumulate forever.
pub fn cleanup_posted_outbox_votes(
    repo: &git2::Repository,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<usize> {
    if !matches!(scope, ChangesRef::Remote { .. }) {
        return Err(anyhow::anyhow!(
            "cleanup_posted_outbox_votes requires a Remote scope"
        ));
    }

    let tracked = crate::forges::github::read_github_vote_ids(repo, change_id, scope)?;
    if tracked.is_empty() {
        return Ok(0);
    }
    let outbox = list_outbox_votes(repo, change_id, scope)?;
    if outbox.is_empty() {
        return Ok(0);
    }

    let encoded = encode_change_id_path(change_id);
    let ref_name = scope.ref_name();
    let mut tree = match repo.find_reference(&ref_name) {
        Ok(r) => r.peel_to_tree()?,
        Err(_) => return Ok(0),
    };

    let mut removed = 0usize;
    for (user, data) in &outbox {
        let posted = match tracked.get(user) {
            Some(p) => p,
            None => continue,
        };
        if posted.state != data.state || posted.sha != data.sha {
            continue;
        }
        let path = std::path::Path::new("outbox/votes")
            .join(&encoded)
            .join(user);
        if tree.get_path(&path).is_ok() {
            tree = josh_core::filter::tree::insert(repo, &tree, &path, git2::Oid::ZERO_SHA1, 0)?;
            removed += 1;
        }
    }

    if removed == 0 {
        return Ok(0);
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
        &format!("cleanup posted outbox votes on {}\n", ref_name),
        &tree,
        &parents,
    )?;

    Ok(removed)
}
