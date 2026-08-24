use crate::change::{Change, encode_change_id_path};
use crate::layout::{ChangesRefData, CommentsByChange};
use crate::refs::ChangesRef;
use crate::store::{get_tree, namespace_filter, value_oid};
use anyhow::anyhow;
use josh_core::cache::Transaction;
use josh_core::filter::tree;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
}

/// The serialized form of a comment; the comment's id is the tree id of this
/// value. `None` fields are omitted from the tree, so the id is stable across
/// layout-irrelevant differences.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommentMeta {
    pub message: String,
    pub file: Option<String>,
    pub location: Option<Location>,
    pub reply_to: Option<String>,
    pub update_of: Option<String>,
}

/// Which comments namespace a write targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentNamespace {
    /// The canonical `comments` tree.
    Default,
    /// The `outbox/comments` queue of a `Remote` ref: pending posts to the
    /// remote, cleaned up on the next fetch that observes them coming back.
    Outbox,
}

impl CommentNamespace {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Default => "comments",
            Self::Outbox => "outbox/comments",
        }
    }

    /// This namespace's map inside a (possibly sparsely populated)
    /// `ChangesRefData`.
    fn of(self, data: &ChangesRefData) -> &CommentsByChange {
        match self {
            Self::Default => &data.comments,
            Self::Outbox => &data.outbox.comments,
        }
    }

    fn of_mut(self, data: &mut ChangesRefData) -> &mut CommentsByChange {
        match self {
            Self::Default => &mut data.comments,
            Self::Outbox => &mut data.outbox.comments,
        }
    }

    /// The write namespace for `scope`: local writes go to `Default`, remote
    /// writes queue in `Outbox`.
    pub fn for_scope(scope: &ChangesRef) -> Self {
        match scope {
            ChangesRef::Local { .. } => Self::Default,
            ChangesRef::Remote { .. } => Self::Outbox,
        }
    }
}

/// Write `meta` into `namespace` on `scope`'s ref. `Outbox` requires a
/// `Remote` scope. Returns the comment id: the tree id of the serialized
/// meta -- content-addressed, so identical metas dedup and the fetch side
/// recomputes the same id.
pub fn write_comment(
    transaction: &Transaction,
    change: &Change,
    meta: &CommentMeta,
    author: Option<&str>,
    timestamp: Option<&str>,
    scope: &ChangesRef,
    namespace: CommentNamespace,
) -> anyhow::Result<String> {
    if namespace == CommentNamespace::Outbox && !matches!(scope, ChangesRef::Remote { .. }) {
        return Err(anyhow!(
            "outbox comments require a Remote scope (got {})",
            scope.ref_name()
        ));
    }
    if meta.message.trim().is_empty() {
        return Err(anyhow!("comment message must not be empty"));
    }

    let change_id = change
        .id()
        .ok_or_else(|| anyhow!("commit {} has no Change-Id", change.commit()))?;

    // The sparse write re-serializes deterministically, so the key matches
    // the value's tree.
    let root = value_oid(transaction, meta)?;
    let content_hash = root.to_string();

    let mut data = ChangesRefData::default();
    namespace.of_mut(&mut data).insert(
        change_id.to_string(),
        [(content_hash.clone(), meta.clone())].into(),
    );
    crate::store::write_filtered(
        transaction,
        scope,
        namespace_filter(namespace.path()),
        &data,
        author,
        timestamp,
    )?;
    Ok(content_hash)
}

/// A comment plus its bookkeeping: `meta` is the serialized form (whose tree
/// id is `id`); author/timestamp are resolved from the ref history on read.
#[derive(Debug, Clone)]
pub struct Comment {
    pub id: String,
    pub meta: CommentMeta,
    pub author: Option<String>,
    pub timestamp: Option<String>,
    /// True when the comment was read from the `outbox/` subtree of a Remote
    /// ref -- i.e. authored locally and not yet observed back from the remote.
    pub pending: bool,
}

pub fn read_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<Comment>> {
    let ref_name = scope.ref_name();
    let Some(head_commit) = transaction.resolve_ref(&ref_name)? else {
        return Ok(Vec::new());
    };

    // Posted first, then pending outbox (only meaningful on Remote refs),
    // each group ordered by comment id for stable display.
    let mut comments = Vec::new();
    for namespace in [CommentNamespace::Default, CommentNamespace::Outbox] {
        let Some(data) = crate::store::read_filtered::<ChangesRefData>(
            transaction,
            scope,
            namespace_filter(namespace.path()),
        )?
        else {
            continue;
        };
        let Some(per_change) = namespace.of(&data).get(change_id) else {
            continue;
        };
        let mut group: Vec<_> = per_change
            .iter()
            .map(|(id, meta)| Comment {
                id: id.clone(),
                meta: meta.clone(),
                author: None,
                timestamp: None,
                pending: namespace == CommentNamespace::Outbox,
            })
            .collect();
        group.sort_by(|a, b| a.id.cmp(&b.id));
        comments.extend(group);
    }

    // Walk history once to resolve author/timestamp. A comment's id is the
    // tree id of its meta, so an entry can only appear, never change: the ids
    // a ref commit introduced are the subtract of its namespace view against
    // its parent's.
    let encoded = encode_change_id_path(change_id);
    let namespaces = [CommentNamespace::Default, CommentNamespace::Outbox].map(|ns| {
        (
            ns,
            namespace_filter(&format!("{}/{}", ns.path(), encoded)),
            std::path::Path::new(ns.path()).join(&encoded),
        )
    });

    for entry in crate::store::walk_ref_history(transaction, head_commit)? {
        if comments.iter().all(|c| c.author.is_some()) {
            break;
        }
        let Ok(parsed) = entry.commit.parsed() else {
            continue;
        };

        for (namespace, filter, dir) in &namespaces {
            for id in
                introduced_comment_ids(transaction, *filter, dir, entry.tree, entry.parent_tree)?
            {
                if let Some(c) = comments.iter_mut().find(|c| {
                    c.author.is_none()
                        && c.pending == (*namespace == CommentNamespace::Outbox)
                        && c.id == id
                }) {
                    c.timestamp = parsed.committer().ok().map(|t| t.seconds().to_string());
                    c.author = parsed
                        .author()
                        .ok()
                        .map(|a| String::from_utf8_lossy(a.email).into_owned());
                }
            }
        }
    }

    Ok(comments)
}

/// Ids of comments under `dir` that `tree` carries and `parent_tree` lacks,
/// both narrowed by `filter`.
fn introduced_comment_ids(
    transaction: &Transaction,
    filter: josh_core::filter::Filter,
    dir: &std::path::Path,
    tree: gix_hash::ObjectId,
    parent_tree: Option<gix_hash::ObjectId>,
) -> anyhow::Result<Vec<String>> {
    let view = |t: gix_hash::ObjectId| {
        josh_core::filter::apply(
            transaction,
            filter,
            josh_core::filter::Rewrite::from_tree(t),
        )
        .map(|r| r.tree_id())
    };
    let cur = view(tree)?;
    let parent = match parent_tree {
        Some(pt) => view(pt)?,
        None => tree::empty_id(),
    };
    let added = tree::subtract(transaction, cur, parent)?;
    let Some(added_dir) = get_tree(transaction, transaction.odb(), added, dir) else {
        return Ok(Vec::new());
    };
    Ok(tree::read_tree(transaction, transaction.odb(), added_dir)?
        .entries()
        .filter_map(|entry| std::str::from_utf8(entry.filename).ok().map(str::to_string))
        .collect())
}

/// Remove specific outbox comment entries by content hash. Used by forge
/// sync paths to drop entries whose posted counterparts have been observed
/// on the forge and stored under `comments/...` already. The hash is the
/// entry's key, so no tree walk is needed; missing entries are a no-op.
pub fn delete_outbox_comments(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
    content_hashes: &[String],
) -> anyhow::Result<()> {
    if content_hashes.is_empty() {
        return Ok(());
    }
    let encoded = encode_change_id_path(change_id);
    let paths: Vec<std::path::PathBuf> = content_hashes
        .iter()
        .map(|hash| {
            std::path::Path::new(CommentNamespace::Outbox.path())
                .join(&encoded)
                .join(hash)
        })
        .collect();
    crate::store::delete_filtered(transaction, &paths, scope)
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

/// A comment fetched from a forge: the forge envelope (its id, the author and
/// timestamp used to attribute the ref commit) around the meta payload to
/// store. `reply_to`, when set, refers to a parent's `forge_id`; it is
/// resolved to the parent's local comment id during storage.
pub struct FetchedComment {
    pub forge_id: String,
    pub author: String,
    pub timestamp: String,
    pub reply_to: Option<String>,
    /// The comment content to store; `reply_to` and `update_of` are set by
    /// the store, not the forge.
    pub meta: CommentMeta,
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
        let mut meta = comment.meta.clone();
        meta.reply_to = comment
            .reply_to
            .as_ref()
            .and_then(|forge_id| id_map.get(forge_id))
            .cloned();

        let hash = write_comment(
            transaction,
            change,
            &meta,
            Some(&comment.author),
            Some(&comment.timestamp),
            scope,
            CommentNamespace::Default,
        )?;
        id_map.insert(comment.forge_id.clone(), hash.clone());
        written.push((hash, comment.forge_id.clone()));
    }

    Ok(written)
}
