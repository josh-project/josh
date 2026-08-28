//! Git object helpers over trait-based object stores.

use std::collections::HashMap;

use gix_object::WriteTo;

/// The raw bytes of a path component, for matching against git tree entry names.
#[cfg(unix)]
pub fn component_bytes(c: &std::ffi::OsStr) -> &[u8] {
    std::os::unix::ffi::OsStrExt::as_bytes(c)
}

/// The raw bytes of a path component, for matching against git tree entry names: their UTF-8
/// encoding. Panics on a component that is not valid Unicode, which cannot name a tree entry.
#[cfg(windows)]
pub fn component_bytes(c: &std::ffi::OsStr) -> &[u8] {
    c.to_str()
        .expect("path component is not valid Unicode")
        .as_bytes()
}

pub mod graph;
pub mod merge;
pub mod revwalk;

pub use graph::{is_descendant_of, merge_base, merge_base_octopus};
pub use merge::{merge_commits, merge_trees};
pub use revwalk::{GenerationFrontier, RangeWalk, RevWalk};

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
/// Follow `oid` to the tree it names, unwrapping annotated tags and commits on the way.
/// Errors when the object is missing or resolves to something that is not tree-ish.
pub fn peel_to_tree(
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
            gix_object::Kind::Tree => return Ok(current),
            gix_object::Kind::Commit => {
                return Ok(
                    gix_object::CommitRefIter::from_bytes(&buffer, gix_hash::Kind::Sha1)
                        .tree_id()?,
                );
            }
            gix_object::Kind::Tag => {
                current = gix_object::TagRefIter::from_bytes(&buffer, gix_hash::Kind::Sha1)
                    .target_id()?;
            }
            kind => {
                return Err(anyhow::anyhow!(
                    "object {} is not tree-ish but a {:?}",
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
        let name = component_bytes(component.as_os_str());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn test_repo() -> (tempfile::TempDir, gix::Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(dir.path()).unwrap();
        (dir, repo)
    }

    fn write_raw(
        repo: &gix::Repository,
        kind: gix_object::Kind,
        data: &[u8],
    ) -> gix_hash::ObjectId {
        gix_object::Write::write_buf(&repo.objects, kind, data).unwrap()
    }

    fn write_raw_tree(
        repo: &gix::Repository,
        entries: &[(&str, &[u8], gix_hash::ObjectId)],
    ) -> gix_hash::ObjectId {
        let mut data = Vec::new();
        for (mode, name, oid) in entries {
            data.extend_from_slice(mode.as_bytes());
            data.push(b' ');
            data.extend_from_slice(name);
            data.push(0);
            data.extend_from_slice(oid.as_bytes());
        }
        write_raw(repo, gix_object::Kind::Tree, &data)
    }

    /// Write raw commit bytes, including non-UTF-8 messages.
    fn commit_with_message(repo: &gix::Repository, message: &[u8]) -> gix_hash::ObjectId {
        let tree = write_raw_tree(repo, &[]);
        let mut data = Vec::new();
        data.extend_from_slice(format!("tree {}\n", tree).as_bytes());
        data.extend_from_slice(b"author t <t@e> 0 +0000\n");
        data.extend_from_slice(b"committer t <t@e> 0 +0000\n\n");
        data.extend_from_slice(message);
        write_raw(repo, gix_object::Kind::Commit, &data)
    }

    /// Commit bytes must remain identical to libgit2's. These object IDs were captured from
    /// libgit2 before its test dependency was removed; every ref josh publishes is
    /// content-addressed, so a formatting difference would renumber history.
    #[test]
    fn write_commit_preserves_libgit2_bytes() {
        let (_dir, repo) = test_repo();
        let empty_tree = write_raw_tree(&repo, &[]);
        let blob = write_blob(&repo.objects, b"x").unwrap();
        let tree_with_blob = write_raw_tree(&repo, &[("100644", b"f", blob)]);

        assert_eq!(
            empty_tree,
            gix_hash::ObjectId::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap()
        );
        assert_eq!(
            tree_with_blob,
            gix_hash::ObjectId::from_str("2561a62d4223eb7660d3b6b02b707048382f4019").unwrap()
        );

        let signature =
            |name: &str, email: &str, seconds: i64, offset_minutes: i32| gix_actor::Signature {
                name: name.into(),
                email: email.into(),
                time: gix_actor::date::Time {
                    seconds,
                    offset: offset_minutes * 60,
                },
            };

        let cases = [
            (
                signature("A", "a@e", 0, 0),
                signature("A", "a@e", 0, 0),
                "subject\n",
                empty_tree,
                [
                    "57576bb048f350bfefc5462a145126ed569a0acb",
                    "e605f51a16159b651aa82171f5a5efbfdd96692a",
                    "b4067ee9a49f8e65bffe7fe097d2d1546348a0c2",
                ],
            ),
            (
                signature("A", "a@e", 1234567890, 120),
                signature("B", "b@e", 1234567899, -330),
                "subject\n\nbody with trailing newline\n",
                tree_with_blob,
                [
                    "3511106bdc497584ea13c5c21b7d620274bee8f9",
                    "a56d8d48bbe9d48c87ae7028246090b76fa5581d",
                    "0cdefeea57471caebdb468cfde5e1c054a4da1ec",
                ],
            ),
            (
                signature("Ünïcode Nàme", "u@e", 42, -60),
                signature("Ünïcode Nàme", "u@e", 42, -60),
                "no trailing newline",
                tree_with_blob,
                [
                    "a7af75bbbbf53f1cea9a1eab5eda7930ebdb9e57",
                    "c54129e4d0368d79d15caf5d9424436b8d57a716",
                    "20e7afd96224d8e1db8bb77d646255296bb2eaf3",
                ],
            ),
            (
                signature("A", "a@e", 99, 0),
                signature("A", "a@e", 99, 0),
                "",
                empty_tree,
                [
                    "c3c5ea7d435de37f631f94befb80cd50eeb36954",
                    "c5052ca1e27fe67bd35350052018049182bd5cb8",
                    "c1a08f5fa90b18367f75988bf6c94bf065b0867a",
                ],
            ),
        ];

        for (author, committer, message, tree, expected) in cases {
            for parent_count in 0..=2 {
                let parent_ids = (0..parent_count)
                    .map(|i| {
                        write_commit(
                            &repo.objects,
                            tree,
                            &[],
                            &author,
                            &committer,
                            &format!("parent {i}"),
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                let got = write_commit(
                    &repo.objects,
                    tree,
                    &parent_ids,
                    &author,
                    &committer,
                    message,
                )
                .unwrap();
                let want = gix_hash::ObjectId::from_str(expected[parent_count]).unwrap();
                assert_eq!(
                    want, got,
                    "commit id diverged for {message:?} with {parent_count} parents"
                );

                let base = CommitData::read(&repo.objects, got).unwrap();
                let copied = write_commit_with_signatures_of(
                    &repo.objects,
                    &base,
                    tree,
                    &parent_ids,
                    message,
                )
                .unwrap();
                assert_eq!(got, copied, "signature-copying writer diverged");
            }
        }
    }

    #[test]
    fn message_trims_leading_newlines() {
        let (_dir, repo) = test_repo();

        // Load-bearing for the tree-level `:unapply`, which parses an oid out of the
        // message.
        let oid = commit_with_message(&repo, b"\n\n0123456789012345678901234567890123456789");
        let data = CommitData::read(&repo.objects, oid).unwrap();
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
        let (_dir, repo) = test_repo();
        let blob = write_blob(&repo.objects, b"x").unwrap();

        let deep = write_raw_tree(&repo, &[("100644", b"leaf.txt", blob)]);
        let sub = write_raw_tree(
            &repo,
            &[("40000", b"deep", deep), ("100644", b"f.txt", blob)],
        );
        // Stored order is deliberately non-canonical ("z.txt" before "a", and the tree "a"
        // before the blob "a.txt" that canonical order would sort first).
        let root = write_raw_tree(
            &repo,
            &[
                ("100644", b"z.txt", blob),
                ("40000", b"a", sub),
                ("100644", b"a.txt", blob),
            ],
        );

        let mut seen = vec![];
        walk_tree_preorder(&repo.objects, root, &mut |parent, entry| {
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
        let (_dir, repo) = test_repo();
        let blob = write_blob(&repo.objects, b"x").unwrap();
        let inner = write_raw_tree(&repo, &[("100644", b"f.txt", blob)]);
        let root = write_raw_tree(&repo, &[("40000", b"bad\xff", inner)]);

        assert!(walk_tree_preorder(&repo.objects, root, &mut |_, _| Ok(())).is_err());
    }
}
