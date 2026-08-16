use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use crate::store::{get_tree, write_changes_tree};
use anyhow::anyhow;
use josh_core::cache::{Expected, Transaction};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CommentMeta {
    pub message: String,
    #[serde(skip)]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<Location>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub update_of: Option<String>,
}

pub fn write_comment(
    transaction: &Transaction,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<String> {
    write_comment_with_commit(transaction, change, meta, author, timestamp, None, scope)
}

pub fn write_comment_with_commit(
    transaction: &Transaction,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    blob_commit_override: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<String> {
    write_comment_inner(
        transaction,
        change,
        meta,
        author,
        timestamp,
        blob_commit_override,
        scope,
        "comments",
    )
}

/// Write a comment into the outbox subtree of a `Remote` ref. Comments here are
/// pending posts to the remote; the cleanup runs on the next fetch when the
/// posted comment is observed coming back from the remote.
pub fn write_outbox_comment(
    transaction: &Transaction,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<String> {
    if !matches!(scope, ChangesRef::Remote { .. }) {
        return Err(anyhow::anyhow!(
            "write_outbox_comment requires a Remote scope (got {})",
            scope.ref_name()
        ));
    }
    write_comment_inner(
        transaction,
        change,
        meta,
        author,
        timestamp,
        None,
        scope,
        "outbox/comments",
    )
}

#[allow(clippy::too_many_arguments)]
fn write_comment_inner(
    transaction: &Transaction,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    blob_commit_override: Option<&str>,
    scope: &ChangesRef,
    path_prefix: &str,
) -> anyhow::Result<String> {
    if meta.message.trim().is_empty() {
        return Err(anyhow::anyhow!("comment message must not be empty"));
    }

    let repo = transaction.repo();
    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let content = serde_json::to_string(meta)?;
    let content_hash =
        git2::Oid::hash_object(git2::ObjectType::Blob, content.as_bytes())?.to_string();
    let blob_oid = repo.blob(content.as_bytes())?;

    let prefix = std::path::Path::new(path_prefix);
    let path = if let Some(ref file) = meta.file {
        let resolve_commit = match blob_commit_override {
            Some(s) => git2::Oid::from_str(s)?,
            None => change.commit(),
        };
        let commit = repo.find_commit(resolve_commit)?;
        let file_blob = commit
            .tree()?
            .get_path(std::path::Path::new(file))?
            .id()
            .to_string();
        prefix
            .join("F")
            .join(encode_change_id_path(&change_id))
            .join(&file_blob)
            .join(file)
            .join(&content_hash)
    } else {
        prefix
            .join("C")
            .join(encode_change_id_path(&change_id))
            .join(&content_hash)
    };
    write_changes_tree(transaction, &path, blob_oid, author, timestamp, scope)?;

    Ok(content_hash)
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub id: String,
    pub message: String,
    pub file: Option<String>,
    pub location: Option<Location>,
    pub reply_to: Option<String>,
    pub update_of: Option<String>,
    pub author: Option<String>,
    pub timestamp: Option<String>,
    /// True when the comment was read from the `outbox/` subtree of a Remote
    /// ref -- i.e. authored locally and not yet observed back from the remote.
    pub pending: bool,
}

pub fn comment_author(
    transaction: &Transaction,
    change: &Change,
    comment_id: &str,
    file: Option<&str>,
    scope: &ChangesRef,
) -> anyhow::Result<(String, String)> {
    let repo = transaction.repo();
    let change_id = match change.id() {
        Some(id) => id,
        None => return Err(anyhow!("change has no Change-Id")),
    };

    let ref_name = scope.ref_name();
    let head = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => repo.find_commit(oid)?,
        None => return Err(anyhow!("{} not found", ref_name)),
    };

    let path = if let Some(f) = file {
        // Find the blob_id for this comment in the current tree.
        let head_tree = head.tree()?;
        let cid_path = std::path::Path::new("comments")
            .join("F")
            .join(encode_change_id_path(change_id));
        let mut found = None;
        if let Some(cid_tree) = get_tree(repo, &head_tree, &cid_path) {
            for blob_entry in cid_tree.iter() {
                let blob_name = blob_entry.name().unwrap_or("");
                let sub = std::path::Path::new(f).join(comment_id);
                let full = cid_path.join(blob_name).join(&sub);
                if head_tree.get_path(&full).is_ok() {
                    found = Some(full);
                    break;
                }
            }
        }
        match found {
            Some(p) => p,
            None => {
                return Err(anyhow!("comment {} not found in {}", comment_id, ref_name));
            }
        }
    } else {
        std::path::Path::new("comments")
            .join("C")
            .join(encode_change_id_path(change_id))
            .join(comment_id)
    };

    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.push(head.id())?;

    for oid in walk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;
        if let Ok(entry) = tree.get_path(&path) {
            // Check if this blob is new (not in parent) or changed.
            let is_new = match commit.parent(0) {
                Ok(parent) => parent
                    .tree()
                    .ok()
                    .and_then(|pt| pt.get_path(&path).ok())
                    .map_or(true, |e| e.id() != entry.id()),
                Err(_) => true,
            };
            if is_new {
                let time = commit.time();
                let date = format!("{}", time.seconds());
                return Ok((commit.author().email().unwrap_or("").to_string(), date));
            }
        }
    }

    Err(anyhow!("comment {} not found in {}", comment_id, ref_name))
}

fn parse_comment_blob(
    repo: &git2::Repository,
    entry: &git2::TreeEntry,
    file: Option<String>,
) -> anyhow::Result<Comment> {
    let id = entry.name().unwrap_or("").to_string();
    let blob = entry.to_object(repo)?.peel_to_blob()?;
    let meta: CommentMeta = serde_json::from_slice(blob.content())?;
    Ok(Comment {
        id,
        message: meta.message,
        file,
        location: meta.location,
        reply_to: meta.reply_to,
        update_of: meta.update_of,
        author: None,
        timestamp: None,
        pending: false,
    })
}

pub fn read_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<Comment>> {
    let repo = transaction.repo();
    let ref_name = scope.ref_name();
    let head_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => repo.find_commit(oid)?,
        None => return Ok(Vec::new()),
    };
    let tree = head_commit.tree()?;

    let mut comments = Vec::new();

    // Posted/fetched: comments/{C,F}/...
    collect_comments_at_prefix(repo, &tree, change_id, "comments", false, &mut comments)?;
    // Pending outbox (only meaningful on Remote refs): outbox/comments/{C,F}/...
    collect_comments_at_prefix(
        repo,
        &tree,
        change_id,
        "outbox/comments",
        true,
        &mut comments,
    )?;

    // Walk history once to resolve author/timestamp for all comments.
    let mut walk = repo.revwalk().unwrap_or_else(|_| repo.revwalk().unwrap());
    let _ = walk.simplify_first_parent();
    let _ = walk.push(head_commit.id());
    'outer: for oid in walk.flatten() {
        if let Ok(commit) = repo.find_commit(oid) {
            let tree = commit.tree().unwrap();
            let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
            for c in &mut comments {
                if c.author.is_some() {
                    continue;
                }
                let prefix = if c.pending {
                    "outbox/comments"
                } else {
                    "comments"
                };
                if c.file.is_none() {
                    let p = std::path::Path::new(prefix)
                        .join("C")
                        .join(encode_change_id_path(change_id))
                        .join(&c.id);
                    if let Ok(entry) = tree.get_path(&p) {
                        let is_new = parent_tree
                            .as_ref()
                            .and_then(|pt| pt.get_path(&p).ok())
                            .map_or(true, |e| e.id() != entry.id());
                        if is_new {
                            let time = commit.time();
                            let ts = time.seconds().to_string();
                            c.author = Some(commit.author().email().unwrap_or("").to_string());
                            c.timestamp = Some(ts);
                        }
                    }
                } else {
                    let cid_path = std::path::Path::new(prefix)
                        .join("F")
                        .join(encode_change_id_path(change_id));
                    if let Some(cid_tree) = get_tree(repo, &tree, &cid_path) {
                        for blob_entry in cid_tree.iter() {
                            let blob_name = blob_entry.name().unwrap_or("");
                            let sub = std::path::Path::new(c.file.as_ref().unwrap()).join(&c.id);
                            let full_path = cid_path.join(blob_name).join(&sub);
                            if let Ok(entry) = tree.get_path(&full_path) {
                                let parent_entry = parent_tree
                                    .as_ref()
                                    .and_then(|pt| pt.get_path(&full_path).ok());
                                let is_new = parent_entry.map_or(true, |e| e.id() != entry.id());
                                if is_new {
                                    let time = commit.time();
                                    let ts = time.seconds().to_string();
                                    c.author =
                                        Some(commit.author().email().unwrap_or("").to_string());
                                    c.timestamp = Some(ts);
                                }
                            }
                        }
                    }
                }
            }
            if comments.iter().all(|c| c.author.is_some()) {
                break 'outer;
            }
        }
    }

    Ok(comments)
}

fn collect_comments_at_prefix(
    repo: &git2::Repository,
    tree: &git2::Tree,
    change_id: &str,
    prefix: &str,
    pending: bool,
    out: &mut Vec<Comment>,
) -> anyhow::Result<()> {
    let root = match get_tree(repo, tree, std::path::Path::new(prefix)) {
        Some(t) => t,
        None => return Ok(()),
    };

    // Non-file: <prefix>/C/<change_id>/
    if let Some(cid_tree) = get_tree(
        repo,
        &root,
        &std::path::Path::new("C").join(encode_change_id_path(change_id)),
    ) {
        for entry in cid_tree.iter() {
            if let Ok(mut c) = parse_comment_blob(repo, &entry, None) {
                c.pending = pending;
                out.push(c);
            }
        }
    }

    // File: <prefix>/F/<change_id>/<blob_id>/<path>/<to>/<file>/<content_hash>
    if let Some(cid_tree) = get_tree(
        repo,
        &root,
        &std::path::Path::new("F").join(encode_change_id_path(change_id)),
    ) {
        for blob_entry in cid_tree.iter() {
            let blob_name = blob_entry.name().unwrap_or("");
            if let Some(blob_tree) = get_tree(repo, &cid_tree, std::path::Path::new(blob_name)) {
                let mut found = Vec::new();
                collect_comments_under_into(
                    repo,
                    &blob_tree,
                    std::path::Path::new(""),
                    &mut found,
                )?;
                for mut c in found {
                    c.pending = pending;
                    out.push(c);
                }
            }
        }
    }

    Ok(())
}

fn collect_comments_under_into(
    repo: &git2::Repository,
    tree: &git2::Tree,
    file_prefix: &std::path::Path,
    out: &mut Vec<Comment>,
) -> anyhow::Result<()> {
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                let subtree = entry.to_object(repo)?.peel_to_tree()?;
                let child_file = file_prefix.join(name);
                collect_comments_under_into(repo, &subtree, &child_file, out)?;
            }
            Some(git2::ObjectType::Blob) => {
                let file = if file_prefix.as_os_str().is_empty() {
                    None
                } else {
                    Some(file_prefix.to_string_lossy().to_string())
                };
                out.push(parse_comment_blob(repo, &entry, file)?);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Remove specific outbox comment entries by content hash. Used by the fetch
/// path to drop entries whose posted counterparts have just come back from
/// the remote. Pass the set of local content hashes whose `gh_ids` entry is
/// known to be reflected in `comments/...` already.
pub fn delete_outbox_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    content_hashes: &[String],
) -> anyhow::Result<usize> {
    if content_hashes.is_empty() {
        return Ok(0);
    }
    let repo = transaction.repo();
    let want: std::collections::HashSet<&str> = content_hashes.iter().map(|s| s.as_str()).collect();

    let ref_name = scope.ref_name();
    let prev_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => repo.find_commit(oid)?,
        None => return Ok(0),
    };
    let mut tree = prev_commit.tree()?;

    let encoded = encode_change_id_path(change_id);
    let mut paths_to_remove: Vec<std::path::PathBuf> = Vec::new();

    // Non-file: outbox/comments/C/<change>/<hash>
    let c_prefix = std::path::Path::new("outbox/comments/C").join(&encoded);
    if let Some(c_tree) = get_tree(repo, &tree, &c_prefix) {
        for entry in c_tree.iter() {
            if let Ok(name) = entry.name() {
                if want.contains(name) {
                    paths_to_remove.push(c_prefix.join(name));
                }
            }
        }
    }

    // File: outbox/comments/F/<change>/<blob_id>/<path>/<to>/<file>/<hash>
    let f_prefix = std::path::Path::new("outbox/comments/F").join(&encoded);
    if let Some(f_tree) = get_tree(repo, &tree, &f_prefix) {
        collect_outbox_file_paths(repo, &f_tree, &f_prefix, &want, &mut paths_to_remove)?;
    }

    if paths_to_remove.is_empty() {
        return Ok(0);
    }

    for path in &paths_to_remove {
        if tree.get_path(path).is_ok() {
            tree = josh_core::filter::tree::insert(repo, &tree, path, git2::Oid::ZERO_SHA1, 0)?;
        }
    }

    let sig = repo.signature()?;
    let msg = format!("cleanup posted outbox comments on {}\n", ref_name);
    let new_oid = repo.commit(None, &sig, &sig, &msg, &tree, &[&prev_commit])?;
    transaction.update_ref(&ref_name, Expected::At(prev_commit.id()), new_oid, &msg)?;

    Ok(paths_to_remove.len())
}

/// Pending (not yet posted to the forge) comments for a change, loaded from
/// the outbox subtree of `scope`, plus the forge IDs of already-posted
/// comments so a publisher can thread replies.
pub struct PendingComments {
    /// Outbox comments with no forge ID mapping yet.
    pub to_post: Vec<Comment>,
    /// local hash -> forge node ID for already-posted comments (reply threading).
    pub posted_ids: std::collections::HashMap<String, String>,
}

pub fn pending_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<PendingComments> {
    // Pending comments live in `outbox/comments/...` on the Remote ref. Anything
    // already under `comments/...` was either fetched from the remote or has
    // already been posted; either way it should not be re-posted.
    let comments: Vec<Comment> = read_comments(transaction, change_id, scope)?
        .into_iter()
        .filter(|c| c.pending)
        .collect();

    let posted_ids = crate::forges::github::read_github_ids(transaction, change_id, scope)?;
    let to_post = comments
        .into_iter()
        .filter(|c| !posted_ids.contains_key(&c.id))
        .collect();

    Ok(PendingComments {
        to_post,
        posted_ids,
    })
}

/// A comment fetched from a forge, in forge-neutral form. `forge_id` is the
/// remote's node/comment ID; `reply_to`, when set, refers to a parent's
/// `forge_id`.
pub struct FetchedComment {
    pub forge_id: String,
    pub author: String,
    pub body: String,
    pub timestamp: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub reply_to: Option<String>,
    pub commit_oid: Option<String>,
}

/// Write comments fetched from a forge into the given changes ref.
/// Returns the number of comments stored.
pub fn store_fetched_comments(
    transaction: &Transaction,
    change: &Change,
    comments: &[FetchedComment],
    scope: &ChangesRef,
) -> anyhow::Result<usize> {
    let mut id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for comment in comments {
        let location = comment
            .path
            .as_ref()
            .zip(comment.line)
            .map(|(_, line)| Location {
                start_line: line as u32,
                end_line: line as u32,
                start_col: 1,
                end_col: u32::MAX,
            });
        let reply_to = comment
            .reply_to
            .as_ref()
            .and_then(|forge_id| id_map.get(forge_id))
            .cloned();
        let meta = CommentMeta {
            message: comment.body.clone(),
            file: comment.path.clone(),
            location,
            reply_to,
            update_of: None,
        };

        let hash = write_comment_with_commit(
            transaction,
            change,
            &meta,
            Some(&comment.author),
            Some(&comment.timestamp),
            comment.commit_oid.as_deref(),
            scope,
        )?;
        // Record the forge ID so this comment is tracked as "already posted".
        if let Some(change_id) = change.id() {
            crate::forges::github::store_github_id(
                transaction,
                change_id,
                &hash,
                &comment.forge_id,
                scope,
            )?;
        }
        id_map.insert(comment.forge_id.clone(), hash);
    }

    // Cleanup: any outbox entry whose `gh_ids[hash]` points at a forge comment
    // we just observed in the fetch can now be dropped — the canonical copy
    // lives under `comments/...` on this ref.
    if let Some(change_id) = change.id() {
        let fetched: std::collections::HashSet<&str> =
            comments.iter().map(|c| c.forge_id.as_str()).collect();
        let gh_ids = crate::forges::github::read_github_ids(transaction, change_id, scope)?;
        let to_remove: Vec<String> = gh_ids
            .into_iter()
            .filter(|(_, forge_id)| fetched.contains(forge_id.as_str()))
            .map(|(local_hash, _)| local_hash)
            .collect();
        delete_outbox_comments(transaction, change_id, scope, &to_remove)?;
    }

    Ok(comments.len())
}

fn collect_outbox_file_paths(
    repo: &git2::Repository,
    tree: &git2::Tree,
    cur: &std::path::Path,
    want: &std::collections::HashSet<&str>,
    out: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    for entry in tree.iter() {
        let name = match entry.name() {
            Ok(n) => n,
            Err(_) => continue,
        };
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                let subtree = entry.to_object(repo)?.peel_to_tree()?;
                collect_outbox_file_paths(repo, &subtree, &cur.join(name), want, out)?;
            }
            Some(git2::ObjectType::Blob) => {
                if want.contains(name) {
                    out.push(cur.join(name));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
