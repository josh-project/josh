//! Git object helpers over trait-based object stores.

use std::collections::HashMap;

use gix_object::WriteTo;

pub mod graph;
pub mod merge;
pub mod revwalk;

pub use graph::{is_descendant_of, merge_base, merge_base_octopus};
pub use merge::{merge_commits, merge_trees};
pub use revwalk::{RangeWalk, RevWalk};

/// Convert a gitoxide object kind to libgit2.
pub fn git2_kind(kind: gix_object::Kind) -> git2::ObjectType {
    match kind {
        gix_object::Kind::Tree => git2::ObjectType::Tree,
        gix_object::Kind::Blob => git2::ObjectType::Blob,
        gix_object::Kind::Commit => git2::ObjectType::Commit,
        gix_object::Kind::Tag => git2::ObjectType::Tag,
    }
}

/// Convert a libgit2 object kind when it represents an object.
pub fn gix_kind(kind: git2::ObjectType) -> Option<gix_object::Kind> {
    match kind {
        git2::ObjectType::Tree => Some(gix_object::Kind::Tree),
        git2::ObjectType::Blob => Some(gix_object::Kind::Blob),
        git2::ObjectType::Commit => Some(gix_object::Kind::Commit),
        git2::ObjectType::Tag => Some(gix_object::Kind::Tag),
        _ => None,
    }
}

/// Convert a SHA-1 object ID to gitoxide.
pub fn gix_oid(oid: git2::Oid) -> gix_hash::ObjectId {
    gix_hash::ObjectId::from_bytes_or_panic(oid.as_bytes())
}

/// Convert a SHA-1 object ID to libgit2.
pub fn git2_oid(oid: &gix_hash::oid) -> git2::Oid {
    git2::Oid::from_bytes(oid.as_bytes()).expect("oid sizes match")
}

/// Hash a blob without writing it.
pub fn hash_blob(data: &[u8]) -> gix_hash::ObjectId {
    gix_object::compute_hash(gix_hash::Kind::Sha1, gix_object::Kind::Blob, data)
        .expect("failed to compute hash")
}

/// Follow `oid` to the commit it names, unwrapping annotated tags on the way. Errors when the
/// object is missing or resolves to something that is not a commit.
pub fn peel_to_commit(
    src: &(impl gix_object::Find + ?Sized),
    oid: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut current = oid;
    let mut buffer = Vec::new();
    loop {
        let data = src
            .try_find(&current, &mut buffer)
            .map_err(|e| anyhow::anyhow!("peel {}: {}", current, e))?
            .ok_or_else(|| anyhow::anyhow!("object {} not found", current))?;
        match data.kind {
            gix_object::Kind::Commit => return Ok(current),
            gix_object::Kind::Tag => {
                current = gix_object::TagRefIter::from_bytes(&buffer, gix_hash::Kind::Sha1)
                    .target_id()?;
            }
            kind => {
                return Err(anyhow::anyhow!(
                    "object {} is not a commit but a {:?}",
                    current,
                    kind
                ));
            }
        }
    }
}

/// The entries of the tree `oid`, owned so several trees can be walked side by side.
/// Errors when the object is missing or is not a tree.
pub fn read_tree_entries(
    src: &(impl gix_object::Find + ?Sized),
    oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<gix_object::tree::Entry>> {
    let mut buffer = Vec::new();
    let data = src
        .try_find(&oid, &mut buffer)
        .map_err(|e| anyhow::anyhow!("read tree {}: {}", oid, e))?
        .ok_or_else(|| anyhow::anyhow!("object {} not found", oid))?;
    if data.kind != gix_object::Kind::Tree {
        return Err(anyhow::anyhow!("object {} is not a tree", oid));
    }
    Ok(
        gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?
            .into_owned()
            .entries,
    )
}

/// Descend `path` from the tree `oid`. `None` covers a missing component and a non-tree
/// entry on the way; the final entry may be of any kind.
pub fn path_entry(
    src: &(impl gix_object::Find + ?Sized),
    oid: gix_hash::ObjectId,
    path: &std::path::Path,
) -> anyhow::Result<Option<gix_object::tree::Entry>> {
    let mut current = oid;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let mut buffer = Vec::new();
        let Some(data) = src
            .try_find(&current, &mut buffer)
            .map_err(|e| anyhow::anyhow!("read tree {}: {}", current, e))?
        else {
            return Ok(None);
        };
        if data.kind != gix_object::Kind::Tree {
            return Ok(None);
        }
        let parsed = gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?;
        let name = std::os::unix::ffi::OsStrExt::as_bytes(component.as_os_str());
        let Some(entry) = parsed.entries.iter().find(|e| e.filename == name) else {
            return Ok(None);
        };
        if components.peek().is_none() {
            return Ok(Some((*entry).into()));
        }
        current = entry.oid.to_owned();
    }
    Ok(None)
}

/// The text of the blob `oid`, or `""` when it is missing, is not a blob, holds a NUL byte or
/// is not valid UTF-8 -- the tolerance the display and script paths want, where a file that
/// cannot be shown is the same as a file that is not there.
pub fn blob_text(src: &(impl gix_object::Find + ?Sized), oid: gix_hash::ObjectId) -> String {
    let mut buffer = Vec::new();
    let Ok(Some(data)) = src.try_find(&oid, &mut buffer) else {
        return String::new();
    };
    if data.kind != gix_object::Kind::Blob || buffer.contains(&0) {
        return String::new();
    }
    String::from_utf8(buffer).unwrap_or_default()
}

/// Preorder tree walk: entries in stored tree order, the callback firing for a tree entry
/// before its descent, `parent` being the slash-separated path of the containing directory
/// (`""` at the root level). Non-tree entry objects are never loaded. Descending into a tree
/// whose name is not UTF-8 errors the walk (`parent` is a `&str`); non-UTF-8 *entry* names
/// are the consumer's concern, the callback sees raw bytes.
///
/// Stored order is load-bearing: `filter::get_link_roots` turns it into the parent order of
/// the merge commits it creates.
pub fn walk_tree_preorder(
    src: &impl gix_object::Find,
    root: gix_hash::ObjectId,
    cb: &mut dyn FnMut(&str, &gix_object::tree::EntryRef<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut path = String::new();
    walk_tree_preorder_inner(src, root, &mut path, cb)
}

fn walk_tree_preorder_inner(
    src: &impl gix_object::Find,
    tree: gix_hash::ObjectId,
    path: &mut String,
    cb: &mut dyn FnMut(&str, &gix_object::tree::EntryRef<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let data = src
        .try_find(&tree, &mut buf)
        .map_err(|e| anyhow::anyhow!("walk_tree_preorder: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("object {} not found", tree))?;
    if data.kind != gix_object::Kind::Tree {
        return Err(anyhow::anyhow!(
            "object {} is not a tree but a {:?}",
            tree,
            data.kind
        ));
    }
    let parsed = gix_object::TreeRef::from_bytes(&buf, gix_hash::Kind::Sha1)?;
    for entry in &parsed.entries {
        cb(path, entry)?;
        if entry.mode.is_tree() {
            let name = std::str::from_utf8(entry.filename)
                .map_err(|_| anyhow::anyhow!("non-utf8 directory name in tree walk"))?;
            let base = path.len();
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(name);
            walk_tree_preorder_inner(src, entry.oid.to_owned(), path, cb)?;
            path.truncate(base);
        }
    }
    Ok(())
}

/// Serialize `entries` as a git tree in exactly the order given and write it straight to `out`
/// (in practice the transaction's memodb-backed facade, so the write is immediately visible
/// to every reader of the repository).
///
/// The order is the caller's responsibility: rebuilds of existing trees preserve the input's
/// entry order byte-for-byte (canonical or not), and freshly inserted entries are placed at
/// their canonical position by the caller. Serialization is manual because gix's tree writer
/// asserts canonical order, which fsck-invalid input trees do not have.
pub fn write_tree_now(
    out: &impl gix_object::Write,
    entries: Vec<gix_object::tree::Entry>,
) -> anyhow::Result<gix_hash::ObjectId> {
    // Exact-fit upper bound: mode (<= 6 octal digits) + space + name + NUL + 20 oid bytes.
    let mut buffer = Vec::with_capacity(
        entries
            .iter()
            .map(|e| 6 + 1 + e.filename.len() + 1 + 20)
            .sum(),
    );
    let mut mode_buf = Default::default();
    for entry in &entries {
        if entry.filename.contains(&0) {
            return Err(anyhow::anyhow!(
                "NUL byte in tree entry name {:?}",
                entry.filename
            ));
        }
        buffer.extend_from_slice(entry.mode.as_bytes(&mut mode_buf));
        buffer.push(b' ');
        buffer.extend_from_slice(&entry.filename);
        buffer.push(0);
        buffer.extend_from_slice(entry.oid.as_bytes());
    }
    let id = out
        .write_buf(gix_object::Kind::Tree, &buffer)
        .map_err(|e| anyhow::anyhow!("write_tree_now: {e}"))?;
    Ok(id)
}

/// Write `data` as a blob to `out`.
pub fn write_blob(out: &impl gix_object::Write, data: &[u8]) -> anyhow::Result<gix_hash::ObjectId> {
    let id = out
        .write_buf(gix_object::Kind::Blob, data)
        .map_err(|e| anyhow::anyhow!("write_blob: {e}"))?;
    Ok(id)
}

/// Serialize a new commit and write it to `out`. The message is written verbatim, so any
/// cleanup is the caller's business.
pub fn write_commit(
    out: &impl gix_object::Write,
    tree: gix_hash::ObjectId,
    parents: &[gix_hash::ObjectId],
    author: &gix_actor::Signature,
    committer: &gix_actor::Signature,
    message: &str,
) -> anyhow::Result<gix_hash::ObjectId> {
    let commit = gix_object::Commit {
        tree,
        parents: parents.to_vec().into(),
        author: author.clone(),
        committer: committer.clone(),
        encoding: None,
        message: message.into(),
        extra_headers: vec![],
    };
    let mut buffer = Vec::with_capacity(commit.size() as usize);
    gix_object::WriteTo::write_to(&commit, &mut buffer)?;
    let id = out
        .write_buf(gix_object::Kind::Commit, &buffer)
        .map_err(|e| anyhow::anyhow!("write_commit: {e}"))?;
    Ok(id)
}

/// Serialize a new commit whose author and committer are taken from `base`, and write it to
/// `out`. Only those two fields carry over -- any extra headers of `base` are dropped.
pub fn write_commit_with_signatures_of(
    out: &impl gix_object::Write,
    base: &CommitData,
    tree: gix_hash::ObjectId,
    parents: &[gix_hash::ObjectId],
    message: &str,
) -> anyhow::Result<gix_hash::ObjectId> {
    let parsed = base.parsed()?;
    let commit = gix_object::Commit {
        tree,
        parents: parents.to_vec().into(),
        author: parsed.author()?.into(),
        committer: parsed.committer()?.into(),
        encoding: None,
        message: message.into(),
        extra_headers: vec![],
    };
    let mut buffer = Vec::with_capacity(commit.size() as usize);
    gix_object::WriteTo::write_to(&commit, &mut buffer)?;
    let id = out
        .write_buf(gix_object::Kind::Commit, &buffer)
        .map_err(|e| anyhow::anyhow!("write_commit: {e}"))?;
    Ok(id)
}

/// A commit's id + raw odb bytes, owned. Send + Sync plain data; never stored in a transaction.
/// Parse-on-demand: `CommitRef`/`CommitRefIter` borrow `&self` and never outlive the frame.
/// The internal representation can become `Arc<[u8]>` later without any signature change.
#[derive(Clone, Debug)]
pub struct CommitData {
    id: gix_hash::ObjectId,
    bytes: Vec<u8>,
}

impl CommitData {
    /// Errors if the object is missing or not a commit. `src` is the transaction's facade in
    /// practice, so unflushed in-memory commits resolve.
    pub fn read(
        src: &impl gix_object::Find,
        oid: gix_hash::ObjectId,
    ) -> anyhow::Result<CommitData> {
        let mut bytes = Vec::new();
        let data = src
            .try_find(&oid, &mut bytes)
            .map_err(|e| anyhow::anyhow!("CommitData::read: {e}"))?
            .ok_or_else(|| anyhow::anyhow!("object {} not found", oid))?;
        if data.kind != gix_object::Kind::Commit {
            return Err(anyhow::anyhow!(
                "object {} is not a commit but a {:?}",
                oid,
                data.kind
            ));
        }
        Ok(CommitData { id: oid, bytes })
    }

    pub fn id(&self) -> gix_hash::ObjectId {
        self.id
    }

    /// The raw serialized commit, exactly as stored in the odb.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Full parse of the raw bytes. Cheap (winnow over a few hundred bytes; no lock, no FFI),
    /// so callers just parse again when they need a second look.
    pub fn parsed(&self) -> anyhow::Result<gix_object::CommitRef<'_>> {
        Ok(gix_object::CommitRef::from_bytes(
            &self.bytes,
            gix_hash::Kind::Sha1,
        )?)
    }

    pub fn tree_id(&self) -> anyhow::Result<gix_hash::ObjectId> {
        let id =
            gix_object::CommitRefIter::from_bytes(&self.bytes, gix_hash::Kind::Sha1).tree_id()?;
        Ok(id)
    }

    /// The stored message verbatim, by way of a full commit parse -- an unparseable commit
    /// is a hard error.
    pub fn message_raw(&self) -> anyhow::Result<&gix_object::bstr::BStr> {
        Ok(self.parsed()?.message)
    }

    /// The message with leading newlines trimmed. Load-bearing for callers that parse the
    /// message as an oid.
    pub fn message(&self) -> anyhow::Result<&gix_object::bstr::BStr> {
        let msg = self.message_raw()?;
        let start = msg.iter().position(|&b| b != b'\n').unwrap_or(msg.len());
        Ok(msg[start..].into())
    }

    /// The commit summary ([`gix_object::CommitRef::message_summary`]); `None` when the
    /// commit does not parse or the summary is not UTF-8.
    pub fn summary(&self) -> Option<String> {
        let commit = self.parsed().ok()?;
        String::from_utf8(commit.message_summary().into_owned().into()).ok()
    }

    /// Binary parent ids read from the parsed commit header id array via
    /// `CommitRefIter::parent_ids`; never does odb lookups.
    pub fn parent_ids(&self) -> impl Iterator<Item = gix_hash::ObjectId> + '_ {
        gix_object::CommitRefIter::from_bytes(&self.bytes, gix_hash::Kind::Sha1).parent_ids()
    }

    pub fn first_parent_id(&self) -> Option<gix_hash::ObjectId> {
        self.parent_ids().next()
    }

    pub fn parent_count(&self) -> usize {
        self.parent_ids().count()
    }
}

/// An in-memory staging store: a pure map for computing content hashes
/// repository-independently (the `Filter::id` use case, which must not touch a repository at
/// all) and for batching writes until an explicit [`flush`](StagingOdb::flush) into an object
/// sink. Reads (via the [`gix_object::Find`] family) see only staged objects.
pub struct StagingOdb {
    /// Staged objects by content hash: `(kind, raw bytes)`, not yet written to the repository.
    pending: HashMap<gix_hash::ObjectId, (gix_object::Kind, Vec<u8>)>,
}

impl StagingOdb {
    pub fn new() -> Self {
        // Pre-seed the empty blob because `write_blob` short-cuts hashing for empty content;
        // seeding keeps the shortcut consistent with flushing (the object is written if missing).
        let mut pending = HashMap::new();
        pending.insert(
            gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1),
            (gix_object::Kind::Blob, Vec::new()),
        );
        Self { pending }
    }

    /// Stage a raw object that is already serialized. Returns its content hash.
    pub fn write_raw(&mut self, kind: gix_object::Kind, data: Vec<u8>) -> gix_hash::ObjectId {
        let hash = gix_object::compute_hash(gix_hash::Kind::Sha1, kind, &data)
            .expect("failed to compute hash");
        self.pending.insert(hash, (kind, data));
        hash
    }

    /// Stage a blob. Empty blobs short-cut to the well-known empty-blob oid.
    pub fn write_blob(&mut self, data: &[u8]) -> gix_hash::ObjectId {
        if data.is_empty() {
            return gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1);
        }
        self.write_raw(gix_object::Kind::Blob, data.to_vec())
    }

    /// Serialize and stage a tree. Entries are written in the order given -- ordering is the
    /// caller's responsibility, because the two orders in use differ: real git trees require
    /// canonical git entry order ([`gix_object::tree::Entry`]'s `Ord`), while persisted filter
    /// trees historically sort by plain filename bytes and must keep hashing identically.
    pub fn write_tree(&mut self, tree: &gix_object::Tree) -> gix_hash::ObjectId {
        let mut buffer = Vec::with_capacity(tree.size() as usize);
        tree.write_to(&mut buffer).expect("failed to write tree");
        self.write_raw(gix_object::Kind::Tree, buffer)
    }

    /// Serialize and stage a commit.
    pub fn write_commit(&mut self, commit: &gix_object::Commit) -> gix_hash::ObjectId {
        let mut buffer = Vec::with_capacity(commit.size() as usize);
        commit
            .write_to(&mut buffer)
            .expect("failed to write commit");
        self.write_raw(gix_object::Kind::Commit, buffer)
    }

    /// Write every staged object to `out`, emptying the staging map. Objects the sink already
    /// has are skipped (see module docs); ids were computed at stage time and are passed on
    /// so the sink need not re-hash.
    pub fn flush(
        &mut self,
        out: &(impl gix_object::Exists + gix_object::Write),
    ) -> anyhow::Result<()> {
        for (oid, (kind, data)) in self.pending.drain() {
            if !out.exists(&oid) {
                out.write_buf_with_known_id(kind, &data, oid)
                    .map_err(|e| anyhow::anyhow!("staging flush: {e}"))?;
            }
        }
        Ok(())
    }
}

impl Default for StagingOdb {
    fn default() -> Self {
        Self::new()
    }
}

impl gix_object::Find for StagingOdb {
    fn try_find<'b>(
        &self,
        id: &gix_hash::oid,
        buffer: &'b mut Vec<u8>,
    ) -> Result<Option<gix_object::Data<'b>>, gix_object::find::Error> {
        let Some((kind, data)) = self.pending.get(id) else {
            return Ok(None);
        };
        buffer.clear();
        buffer.extend_from_slice(data);
        Ok(Some(gix_object::Data {
            kind: *kind,
            object_hash: id.kind(),
            data: buffer,
        }))
    }
}

impl gix_object::FindHeader for StagingOdb {
    fn try_header(
        &self,
        id: &gix_hash::oid,
    ) -> Result<Option<gix_object::Header>, gix_object::find::Error> {
        Ok(self.pending.get(id).map(|(kind, data)| gix_object::Header {
            kind: *kind,
            size: data.len() as u64,
        }))
    }
}

impl gix_object::Exists for StagingOdb {
    fn exists(&self, id: &gix_hash::oid) -> bool {
        self.pending.contains_key(id)
    }
}

/// [`gix_object`] object access over a bare `git2::Odb`: reads/exists resolve through the
/// odb's backends (including a registered memodb and alternates), writes go through
/// `git_odb_write`. The bridge for code that has a git2 odb handle but no memodb store —
/// walker unit tests, and `persist::as_tree` on repositories below the cache stack.
pub struct Git2Odb<'a>(pub &'a git2::Odb<'a>);

impl gix_object::Find for Git2Odb<'_> {
    fn try_find<'b>(
        &self,
        id: &gix_hash::oid,
        buffer: &'b mut Vec<u8>,
    ) -> Result<Option<gix_object::Data<'b>>, gix_object::find::Error> {
        let obj = match self.0.read(git2_oid(id)) {
            Ok(obj) => obj,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(Box::new(e)),
        };
        let Some(kind) = gix_kind(obj.kind()) else {
            return Ok(None);
        };
        buffer.clear();
        buffer.extend_from_slice(obj.data());
        Ok(Some(gix_object::Data {
            kind,
            object_hash: id.kind(),
            data: buffer,
        }))
    }
}

impl gix_object::FindHeader for Git2Odb<'_> {
    fn try_header(
        &self,
        id: &gix_hash::oid,
    ) -> Result<Option<gix_object::Header>, gix_object::find::Error> {
        let (size, kind) = match self.0.read_header(git2_oid(id)) {
            Ok(h) => h,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(Box::new(e)),
        };
        let Some(kind) = gix_kind(kind) else {
            return Ok(None);
        };
        Ok(Some(gix_object::Header {
            kind,
            size: size as u64,
        }))
    }
}

impl gix_object::Exists for Git2Odb<'_> {
    fn exists(&self, id: &gix_hash::oid) -> bool {
        self.0.exists(git2_oid(id))
    }
}

impl gix_object::Write for Git2Odb<'_> {
    fn write_buf(
        &self,
        object: gix_object::Kind,
        from: &[u8],
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        let oid = self.0.write(git2_kind(object), from)?;
        Ok(gix_oid(oid))
    }

    fn write_buf_with_known_id(
        &self,
        object: gix_object::Kind,
        from: &[u8],
        _id: gix_hash::ObjectId,
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        // libgit2 re-hashes on write regardless; the result is the same id.
        self.write_buf(object, from)
    }

    fn write_stream(
        &self,
        kind: gix_object::Kind,
        size: u64,
        from: &mut dyn std::io::Read,
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        let mut data = Vec::with_capacity(size as usize);
        from.read_to_end(&mut data)?;
        self.write_buf(kind, &data)
    }

    fn write_stream_with_known_id(
        &self,
        kind: gix_object::Kind,
        size: u64,
        from: &mut dyn std::io::Read,
        _id: gix_hash::ObjectId,
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        self.write_stream(kind, size, from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write raw commit bytes, including non-UTF-8 messages.
    fn commit_with_message(repo: &git2::Repository, message: &[u8]) -> gix_hash::ObjectId {
        let tree = repo.treebuilder(None).unwrap().write().unwrap();
        let mut data = Vec::new();
        data.extend_from_slice(format!("tree {}\n", tree).as_bytes());
        data.extend_from_slice(b"author t <t@e> 0 +0000\n");
        data.extend_from_slice(b"committer t <t@e> 0 +0000\n\n");
        data.extend_from_slice(message);
        gix_oid(
            repo.odb()
                .unwrap()
                .write(git2::ObjectType::Commit, &data)
                .unwrap(),
        )
    }

    /// Commits written here must be byte-identical to the ones libgit2 writes for the same
    /// inputs -- every ref josh publishes is content-addressed, so a formatting difference
    /// would renumber history.
    #[test]
    fn write_commit_matches_git2() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(dir.path()).unwrap();
        let odb = repo.odb().unwrap();

        let tree = gix_oid(repo.treebuilder(None).unwrap().write().unwrap());
        let blob = repo.blob(b"x").unwrap();
        let mut b = repo.treebuilder(None).unwrap();
        b.insert("f", blob, 0o100644).unwrap();
        let tree2 = gix_oid(b.write().unwrap());

        let sig = |name: &str, email: &str, secs: i64, offset: i32| {
            git2::Signature::new(name, email, &git2::Time::new(secs, offset)).unwrap()
        };
        let actor = |sig: &git2::Signature<'_>| {
            let when = sig.when();
            gix_actor::Signature {
                name: sig.name_bytes().into(),
                email: sig.email_bytes().into(),
                time: gix_actor::date::Time {
                    seconds: when.seconds(),
                    offset: i32::from(when.offset_minutes()) * 60,
                },
            }
        };

        let cases: Vec<(git2::Signature, git2::Signature, &str, gix_hash::ObjectId)> = vec![
            (
                sig("A", "a@e", 0, 0),
                sig("A", "a@e", 0, 0),
                "subject\n",
                tree,
            ),
            (
                sig("A", "a@e", 1234567890, 120),
                sig("B", "b@e", 1234567899, -330),
                "subject\n\nbody with trailing newline\n",
                tree2,
            ),
            (
                sig("Ünïcode Nàme", "u@e", 42, -60),
                sig("Ünïcode Nàme", "u@e", 42, -60),
                "no trailing newline",
                tree2,
            ),
            (sig("A", "a@e", 99, 0), sig("A", "a@e", 99, 0), "", tree),
        ];

        for (author, committer, message, tree) in &cases {
            for parents in [vec![], vec![0usize], vec![0, 1]] {
                // Build the parent commits with git2 so both writers see identical inputs.
                let parent_ids: Vec<gix_hash::ObjectId> = parents
                    .iter()
                    .map(|i| {
                        let t = repo.find_tree(git2_oid(tree)).unwrap();
                        gix_oid(
                            repo.commit(None, author, committer, &format!("parent {i}"), &t, &[])
                                .unwrap(),
                        )
                    })
                    .collect();
                let parent_commits: Vec<git2::Commit> = parent_ids
                    .iter()
                    .map(|id| repo.find_commit(git2_oid(id)).unwrap())
                    .collect();
                let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();

                let want = gix_oid(
                    repo.commit(
                        None,
                        author,
                        committer,
                        message,
                        &repo.find_tree(git2_oid(tree)).unwrap(),
                        &parent_refs,
                    )
                    .unwrap(),
                );
                let author_actor = actor(author);
                let committer_actor = actor(committer);
                let got = write_commit(
                    &Git2Odb(&odb),
                    *tree,
                    &parent_ids,
                    &author_actor,
                    &committer_actor,
                    message,
                )
                .unwrap();
                assert_eq!(
                    want,
                    got,
                    "commit id diverged for {:?} with {} parents",
                    message,
                    parent_ids.len()
                );

                // The signature-copying variant reproduces the same commit when the
                // signatures it copies are the ones git2 was given.
                let base = CommitData::read(&Git2Odb(&odb), want).unwrap();
                let copied = write_commit_with_signatures_of(
                    &Git2Odb(&odb),
                    &base,
                    *tree,
                    &parent_ids,
                    message,
                )
                .unwrap();
                assert_eq!(want, copied, "signature-copying writer diverged");
            }
        }
    }

    #[test]
    fn message_trims_leading_newlines() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(dir.path()).unwrap();
        let odb = repo.odb().unwrap();

        // Load-bearing for the tree-level `:unapply`, which parses an oid out of the
        // message.
        let oid = commit_with_message(&repo, b"\n\n0123456789012345678901234567890123456789");
        let data = CommitData::read(&Git2Odb(&odb), oid).unwrap();
        assert_eq!(
            data.message().unwrap(),
            "0123456789012345678901234567890123456789"
        );
        assert_eq!(
            data.summary().unwrap(),
            "0123456789012345678901234567890123456789"
        );
    }

    /// The walk yields entries in stored tree order (never canonically sorted), fires for a
    /// tree entry before descending into it, and hands the callback the containing
    /// directory's slash-separated path.
    #[test]
    fn walk_tree_preorder_yields_stored_order_paths() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(dir.path()).unwrap();
        let odb = repo.odb().unwrap();
        let blob = gix_oid(repo.blob(b"x").unwrap());

        let write_tree = |entries: &[(&str, &str, gix_hash::ObjectId)]| -> gix_hash::ObjectId {
            let mut data = Vec::new();
            for (mode, name, oid) in entries {
                data.extend_from_slice(mode.as_bytes());
                data.push(b' ');
                data.extend_from_slice(name.as_bytes());
                data.push(0);
                data.extend_from_slice(oid.as_bytes());
            }
            gix_oid(
                repo.odb()
                    .unwrap()
                    .write(git2::ObjectType::Tree, &data)
                    .unwrap(),
            )
        };

        let deep = write_tree(&[("100644", "leaf.txt", blob)]);
        let sub = write_tree(&[("40000", "deep", deep), ("100644", "f.txt", blob)]);
        // Stored order is deliberately non-canonical ("z.txt" before "a", and the tree "a"
        // before the blob "a.txt" that canonical order would sort first).
        let root = write_tree(&[
            ("100644", "z.txt", blob),
            ("40000", "a", sub),
            ("100644", "a.txt", blob),
        ]);

        let mut seen = vec![];
        walk_tree_preorder(&Git2Odb(&odb), root, &mut |parent, entry| {
            seen.push(format!(
                "{}|{}",
                parent,
                std::str::from_utf8(entry.filename).unwrap()
            ));
            Ok(())
        })
        .unwrap();

        assert_eq!(
            seen,
            [
                "|z.txt",
                "|a",
                "a|deep",
                "a/deep|leaf.txt",
                "a|f.txt",
                "|a.txt",
            ]
        );
    }

    /// A tree whose name is not UTF-8 cannot be part of the callback's path argument, so
    /// descending into it errors the whole walk.
    #[test]
    fn walk_tree_preorder_errors_on_non_utf8_directory() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(dir.path()).unwrap();
        let odb = repo.odb().unwrap();
        let blob = gix_oid(repo.blob(b"x").unwrap());

        let mut inner = repo.treebuilder(None).unwrap();
        inner.insert("f.txt", git2_oid(&blob), 0o100644).unwrap();
        let inner = inner.write().unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(b"40000 bad\xff");
        data.push(0);
        data.extend_from_slice(inner.as_bytes());
        let root = gix_oid(
            repo.odb()
                .unwrap()
                .write(git2::ObjectType::Tree, &data)
                .unwrap(),
        );

        assert!(walk_tree_preorder(&Git2Odb(&odb), root, &mut |_, _| Ok(())).is_err());
    }
}
