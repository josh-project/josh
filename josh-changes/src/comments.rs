use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use crate::store::{get_tree, write_changes_tree};
use anyhow::anyhow;
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::memodb::Odb;
use josh_core::objects;

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

    let change_id = change
        .id()
        .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;

    let content = serde_json::to_string(meta)?;
    let content_hash =
        git2::Oid::hash_object(git2::ObjectType::Blob, content.as_bytes())?.to_string();
    let odb = transaction.odb()?;
    let blob_oid = objects::write_blob(&odb, content.as_bytes())?;

    let prefix = std::path::Path::new(path_prefix);
    let path = if let Some(ref file) = meta.file {
        let resolve_commit = match blob_commit_override {
            Some(s) => git2::Oid::from_str(s)?,
            None => change.commit(),
        };
        let commit_tree = objects::CommitData::read(&odb, resolve_commit)?.tree_id()?;
        let file_blob =
            tree::get_path_entry(transaction, &odb, commit_tree, std::path::Path::new(file))?
                .ok_or_else(|| anyhow!("no such path: {}", file))?
                .oid
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
    let change_id = match change.id() {
        Some(id) => id,
        None => return Err(anyhow!("change has no Change-Id")),
    };

    let odb = transaction.odb()?;
    let ref_name = scope.ref_name();
    let head = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => oid,
        None => return Err(anyhow!("{} not found", ref_name)),
    };

    let path = if let Some(f) = file {
        // Find the blob_id for this comment in the current tree.
        let head_tree = objects::CommitData::read(&odb, head)?.tree_id()?;
        let cid_path = std::path::Path::new("comments")
            .join("F")
            .join(encode_change_id_path(change_id));
        let mut found = None;
        if let Some(cid_tree) = get_tree(transaction, &odb, head_tree, &cid_path) {
            for blob_entry in tree::read_tree(transaction, &odb, cid_tree)?.entries() {
                let blob_name = std::str::from_utf8(blob_entry.filename).unwrap_or("");
                let sub = std::path::Path::new(f).join(comment_id);
                let full = cid_path.join(blob_name).join(&sub);
                if matches!(
                    tree::get_path_entry(transaction, &odb, head_tree, &full),
                    Ok(Some(_))
                ) {
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

    let mut walk = objects::RevWalk::new(&odb);
    walk.simplify_first_parent();
    walk.push(head)?;

    for oid in walk.into_topo_vec(|_| false)? {
        let commit = objects::CommitData::read(&odb, oid)?;
        let tree = commit.tree_id()?;
        if let Ok(Some(entry)) = tree::get_path_entry(transaction, &odb, tree, &path) {
            // The commit that introduced or last changed this blob authored the comment.
            let is_new = match commit.first_parent_id() {
                Some(parent) => josh_core::git::read_tree_id(&odb, parent)
                    .ok()
                    .and_then(|pt| {
                        tree::get_path_entry(transaction, &odb, pt, &path)
                            .ok()
                            .flatten()
                    })
                    .is_none_or(|e| e.oid != entry.oid),
                None => true,
            };
            if is_new {
                let parsed = commit.parsed()?;
                let date = format!("{}", parsed.committer()?.seconds());
                let email = parsed.author()?.email;
                return Ok((String::from_utf8_lossy(email).into_owned(), date));
            }
        }
    }

    Err(anyhow!("comment {} not found in {}", comment_id, ref_name))
}

fn parse_comment_blob(
    odb: &Odb,
    name: &[u8],
    blob_oid: git2::Oid,
    file: Option<String>,
) -> anyhow::Result<Comment> {
    let id = String::from_utf8_lossy(name).into_owned();
    let blob =
        tree::blob_bytes(odb, blob_oid).ok_or_else(|| anyhow!("not a blob: {}", blob_oid))?;
    let meta: CommentMeta = serde_json::from_slice(&blob)?;
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
    let odb = transaction.odb()?;
    let ref_name = scope.ref_name();
    let head_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => oid,
        None => return Ok(Vec::new()),
    };
    let tree = objects::CommitData::read(&odb, head_commit)?.tree_id()?;

    let mut comments = Vec::new();

    // Posted/fetched: comments/{C,F}/...
    collect_comments_at_prefix(
        transaction,
        &odb,
        tree,
        change_id,
        "comments",
        false,
        &mut comments,
    )?;
    // Pending outbox (only meaningful on Remote refs): outbox/comments/{C,F}/...
    collect_comments_at_prefix(
        transaction,
        &odb,
        tree,
        change_id,
        "outbox/comments",
        true,
        &mut comments,
    )?;

    // Walk history once to resolve author/timestamp for all comments.
    let mut walk = objects::RevWalk::new(&odb);
    walk.simplify_first_parent();
    let _ = walk.push(head_commit);
    'outer: for oid in walk.into_topo_vec(|_| false)?.into_iter() {
        if let Ok(commit) = objects::CommitData::read(&odb, oid) {
            let Ok(tree) = commit.tree_id() else { continue };
            let parent_tree = commit
                .first_parent_id()
                .and_then(|p| josh_core::git::read_tree_id(&odb, p).ok());
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
                    if let Ok(Some(entry)) = tree::get_path_entry(transaction, &odb, tree, &p) {
                        let is_new = parent_tree
                            .and_then(|pt| {
                                tree::get_path_entry(transaction, &odb, pt, &p)
                                    .ok()
                                    .flatten()
                            })
                            .is_none_or(|e| e.oid != entry.oid);
                        if is_new && let Ok(parsed) = commit.parsed() {
                            c.timestamp = parsed.committer().ok().map(|t| t.seconds().to_string());
                            c.author = parsed
                                .author()
                                .ok()
                                .map(|a| String::from_utf8_lossy(a.email).into_owned());
                        }
                    }
                } else {
                    let cid_path = std::path::Path::new(prefix)
                        .join("F")
                        .join(encode_change_id_path(change_id));
                    if let Some(cid_tree) = get_tree(transaction, &odb, tree, &cid_path)
                        && let Ok(reader) = tree::read_tree(transaction, &odb, cid_tree)
                    {
                        for blob_entry in reader.entries() {
                            let blob_name = std::str::from_utf8(blob_entry.filename).unwrap_or("");
                            let sub = std::path::Path::new(c.file.as_ref().unwrap()).join(&c.id);
                            let full_path = cid_path.join(blob_name).join(&sub);
                            if let Ok(Some(entry)) =
                                tree::get_path_entry(transaction, &odb, tree, &full_path)
                            {
                                let parent_entry = parent_tree.and_then(|pt| {
                                    tree::get_path_entry(transaction, &odb, pt, &full_path)
                                        .ok()
                                        .flatten()
                                });
                                let is_new = parent_entry.is_none_or(|e| e.oid != entry.oid);
                                if is_new && let Ok(parsed) = commit.parsed() {
                                    c.timestamp =
                                        parsed.committer().ok().map(|t| t.seconds().to_string());
                                    c.author = parsed
                                        .author()
                                        .ok()
                                        .map(|a| String::from_utf8_lossy(a.email).into_owned());
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

#[allow(clippy::too_many_arguments)]
fn collect_comments_at_prefix(
    transaction: &Transaction,
    odb: &Odb,
    tree: git2::Oid,
    change_id: &str,
    prefix: &str,
    pending: bool,
    out: &mut Vec<Comment>,
) -> anyhow::Result<()> {
    let root = match get_tree(transaction, odb, tree, std::path::Path::new(prefix)) {
        Some(t) => t,
        None => return Ok(()),
    };

    // Non-file: <prefix>/C/<change_id>/
    if let Some(cid_tree) = get_tree(
        transaction,
        odb,
        root,
        &std::path::Path::new("C").join(encode_change_id_path(change_id)),
    ) {
        for entry in tree::read_tree(transaction, odb, cid_tree)?.entries() {
            if let Ok(mut c) =
                parse_comment_blob(odb, entry.filename, objects::git2_oid(&entry.oid), None)
            {
                c.pending = pending;
                out.push(c);
            }
        }
    }

    // File: <prefix>/F/<change_id>/<blob_id>/<path>/<to>/<file>/<content_hash>
    if let Some(cid_tree) = get_tree(
        transaction,
        odb,
        root,
        &std::path::Path::new("F").join(encode_change_id_path(change_id)),
    ) {
        for blob_entry in tree::read_tree(transaction, odb, cid_tree)?.entries() {
            let blob_name = std::str::from_utf8(blob_entry.filename).unwrap_or("");
            if let Some(blob_tree) =
                get_tree(transaction, odb, cid_tree, std::path::Path::new(blob_name))
            {
                let mut found = Vec::new();
                collect_comments_under_into(
                    transaction,
                    odb,
                    blob_tree,
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
    transaction: &Transaction,
    odb: &Odb,
    tree: git2::Oid,
    file_prefix: &std::path::Path,
    out: &mut Vec<Comment>,
) -> anyhow::Result<()> {
    for entry in tree::read_tree(transaction, odb, tree)?.entries() {
        let name = std::str::from_utf8(entry.filename).unwrap_or("");
        if entry.mode.is_tree() {
            let child_file = file_prefix.join(name);
            collect_comments_under_into(
                transaction,
                odb,
                objects::git2_oid(&entry.oid),
                &child_file,
                out,
            )?;
        } else if !entry.mode.is_commit() {
            let file = if file_prefix.as_os_str().is_empty() {
                None
            } else {
                Some(file_prefix.to_string_lossy().to_string())
            };
            out.push(parse_comment_blob(
                odb,
                entry.filename,
                objects::git2_oid(&entry.oid),
                file,
            )?);
        }
    }
    Ok(())
}

/// Remove specific outbox comment entries by content hash. Used by forge
/// sync paths to drop entries whose posted counterparts have been observed
/// on the forge and stored under `comments/...` already.
pub fn delete_outbox_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    content_hashes: &[String],
) -> anyhow::Result<usize> {
    if content_hashes.is_empty() {
        return Ok(0);
    }
    let odb = transaction.odb()?;
    let want: std::collections::HashSet<&str> = content_hashes.iter().map(|s| s.as_str()).collect();

    let ref_name = scope.ref_name();
    let prev_commit = match transaction.resolve_ref(&ref_name)? {
        Some(oid) => oid,
        None => return Ok(0),
    };
    let mut tree = objects::CommitData::read(&odb, prev_commit)?.tree_id()?;

    let encoded = encode_change_id_path(change_id);
    let mut paths_to_remove: Vec<std::path::PathBuf> = Vec::new();

    // Non-file: outbox/comments/C/<change>/<hash>
    let c_prefix = std::path::Path::new("outbox/comments/C").join(&encoded);
    if let Some(c_tree) = get_tree(transaction, &odb, tree, &c_prefix) {
        for entry in tree::read_tree(transaction, &odb, c_tree)?.entries() {
            if let Ok(name) = std::str::from_utf8(entry.filename) {
                if want.contains(name) {
                    paths_to_remove.push(c_prefix.join(name));
                }
            }
        }
    }

    // File: outbox/comments/F/<change>/<blob_id>/<path>/<to>/<file>/<hash>
    let f_prefix = std::path::Path::new("outbox/comments/F").join(&encoded);
    if let Some(f_tree) = get_tree(transaction, &odb, tree, &f_prefix) {
        collect_outbox_file_paths(
            transaction,
            &odb,
            f_tree,
            &f_prefix,
            &want,
            &mut paths_to_remove,
        )?;
    }

    if paths_to_remove.is_empty() {
        return Ok(0);
    }

    for path in &paths_to_remove {
        if matches!(
            tree::get_path_entry(transaction, &odb, tree, path),
            Ok(Some(_))
        ) {
            tree = tree::insert_oid(&odb, tree, path, git2::Oid::ZERO_SHA1, 0)?;
        }
    }

    let sig = transaction.signature()?;
    let msg = format!("cleanup posted outbox comments on {}\n", ref_name);
    let new_oid = objects::write_commit(&odb, tree, &[prev_commit], &sig, &sig, &msg)?;
    transaction.update_ref(&ref_name, Expected::At(prev_commit), new_oid, &msg)?;

    Ok(paths_to_remove.len())
}

/// Pending (not yet posted to the forge) comments for a change, loaded from
/// the outbox subtree of `scope`, plus the forge IDs of already-posted
/// comments so a publisher can thread replies.
///
/// Constructed by forge crates (e.g. `josh_github_changes::pending_comments`),
/// which combine [`read_comments`] with their forge-ID tracking.
pub struct PendingComments {
    /// Outbox comments with no forge ID mapping yet.
    pub to_post: Vec<Comment>,
    /// local hash -> forge node ID for already-posted comments (reply threading).
    pub posted_ids: std::collections::HashMap<String, String>,
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
/// Returns the `(local hash, forge ID)` pairs written, in fetch order.
///
/// Recording which local hash maps to which forge ID (so the comment is
/// tracked as already posted) and dropping outbox entries observed in the
/// fetch are the caller's job; see `josh_github_changes::record_fetched_comments`
/// for the GitHub composition.
pub fn store_fetched_comments(
    transaction: &Transaction,
    change: &Change,
    comments: &[FetchedComment],
    scope: &ChangesRef,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut written = Vec::with_capacity(comments.len());
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
        id_map.insert(comment.forge_id.clone(), hash.clone());
        written.push((hash, comment.forge_id.clone()));
    }

    Ok(written)
}

fn collect_outbox_file_paths(
    transaction: &Transaction,
    odb: &Odb,
    tree: git2::Oid,
    cur: &std::path::Path,
    want: &std::collections::HashSet<&str>,
    out: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    for entry in tree::read_tree(transaction, odb, tree)?.entries() {
        let name = match std::str::from_utf8(entry.filename) {
            Ok(n) => n,
            Err(_) => continue,
        };
        match () {
            _ if entry.mode.is_tree() => {
                collect_outbox_file_paths(
                    transaction,
                    odb,
                    objects::git2_oid(&entry.oid),
                    &cur.join(name),
                    want,
                    out,
                )?;
            }
            _ if !entry.mode.is_commit() => {
                if want.contains(name) {
                    out.push(cur.join(name));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
