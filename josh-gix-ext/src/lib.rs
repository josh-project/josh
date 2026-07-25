//! In-memory object staging over a single object database.
//!
//! This is the transition vehicle for the incremental git2 -> gix port: all gix-object compute
//! (tree construction, commit parsing and serialization, hashing) works against this adapter,
//! which stages written objects in memory and reads through to the one repository object
//! database. At no point does a second repository handle perform I/O -- the lesson from the
//! reverted side-by-side gitoxide integration (cd6dc206) is that gix is used for pure in-memory
//! compute while a single ODB owns all I/O.
//!
//! Objects are staged as raw `(kind, bytes)` pairs keyed by their content hash, computed with
//! [`gix_object::compute_hash`] -- no repository access, no zlib, no filesystem. [`flush`] batch
//! writes the staged objects to the repository ODB at an explicit boundary, skipping objects that
//! already exist (on some platforms `exists()` is cheaper in terms of I/O than `write()`, because
//! `write()` updates the file access time in the loose object backend).
//!
//! The adapter implements [`gix_object::Find`] (and friends), so gix readers -- `TreeRef`
//! parsing, `CommitRefIter`, the tree editor, the topo walk -- see staged-but-unflushed objects
//! and disk objects through one interface.
//!
//! [`flush`]: StagingOdb::flush

use std::collections::HashMap;

use gix_object::WriteTo;

/// Map the kind of a raw object between the two libraries. Infallible: both enums cover exactly
/// the four git object kinds.
pub fn git2_kind(kind: gix_object::Kind) -> git2::ObjectType {
    match kind {
        gix_object::Kind::Tree => git2::ObjectType::Tree,
        gix_object::Kind::Blob => git2::ObjectType::Blob,
        gix_object::Kind::Commit => git2::ObjectType::Commit,
        gix_object::Kind::Tag => git2::ObjectType::Tag,
    }
}

/// See [`git2_kind`]. Fails only for `Any`/`Ref`, which are not object kinds.
pub fn gix_kind(kind: git2::ObjectType) -> Option<gix_object::Kind> {
    match kind {
        git2::ObjectType::Tree => Some(gix_object::Kind::Tree),
        git2::ObjectType::Blob => Some(gix_object::Kind::Blob),
        git2::ObjectType::Commit => Some(gix_object::Kind::Commit),
        git2::ObjectType::Tag => Some(gix_object::Kind::Tag),
        _ => None,
    }
}

/// Zero-cost oid conversion: both libraries use the same 20-byte binary representation.
pub fn gix_oid(oid: git2::Oid) -> gix_hash::ObjectId {
    gix_hash::ObjectId::from_bytes_or_panic(oid.as_bytes())
}

/// See [`gix_oid`].
pub fn git2_oid(oid: &gix_hash::oid) -> git2::Oid {
    git2::Oid::from_bytes(oid.as_bytes()).expect("oid sizes match")
}

/// An in-memory staging store over an optional read-through object database.
///
/// Writes stage objects in a hash map; reads (via the [`gix_object::Find`] family) consult the
/// staged objects first and fall back to the repository ODB when one is attached. Constructed
/// without an ODB it is a pure store for computing content hashes repository-independently (the
/// `Filter::id` use case, which must not touch a repository at all).
pub struct StagingOdb<'a> {
    /// Staged objects by content hash: `(kind, raw bytes)`, not yet written to the repository.
    pending: HashMap<gix_hash::ObjectId, (gix_object::Kind, Vec<u8>)>,
    /// Read-through target, when reads are needed.
    odb: Option<git2::Odb<'a>>,
}

impl<'a> StagingOdb<'a> {
    /// A pure in-memory store without read-through: reads only see staged objects.
    pub fn new() -> Self {
        // Pre-seed the empty blob because `write_blob` short-cuts hashing for empty content;
        // seeding keeps the shortcut consistent with flushing (the object is written if missing).
        let mut pending = HashMap::new();
        pending.insert(
            gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1),
            (gix_object::Kind::Blob, Vec::new()),
        );
        Self { pending, odb: None }
    }

    /// A staging store reading through to `odb` for objects that are not staged.
    pub fn with_odb(odb: git2::Odb<'a>) -> Self {
        Self {
            odb: Some(odb),
            ..Self::new()
        }
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

    /// Write every staged object to `odb`, emptying the staging map. Objects the ODB already
    /// has are skipped (see module docs).
    pub fn flush(&mut self, odb: &git2::Odb) -> anyhow::Result<()> {
        for (oid, (kind, data)) in self.pending.drain() {
            let oid = git2_oid(&oid);
            if !odb.exists(oid) {
                odb.write(git2_kind(kind), &data)?;
            }
        }
        Ok(())
    }
}

impl Default for StagingOdb<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl gix_object::Find for StagingOdb<'_> {
    fn try_find<'b>(
        &self,
        id: &gix_hash::oid,
        buffer: &'b mut Vec<u8>,
    ) -> Result<Option<gix_object::Data<'b>>, gix_object::find::Error> {
        if let Some((kind, data)) = self.pending.get(id) {
            buffer.clear();
            buffer.extend_from_slice(data);
            return Ok(Some(gix_object::Data {
                kind: *kind,
                data: buffer,
            }));
        }
        let Some(odb) = &self.odb else {
            return Ok(None);
        };
        let obj = match odb.read(git2_oid(id)) {
            Ok(obj) => obj,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(Box::new(e)),
        };
        let Some(kind) = gix_kind(obj.kind()) else {
            return Ok(None);
        };
        buffer.clear();
        buffer.extend_from_slice(obj.data());
        Ok(Some(gix_object::Data { kind, data: buffer }))
    }
}

impl gix_object::FindHeader for StagingOdb<'_> {
    fn try_header(
        &self,
        id: &gix_hash::oid,
    ) -> Result<Option<gix_object::Header>, gix_object::find::Error> {
        if let Some((kind, data)) = self.pending.get(id) {
            return Ok(Some(gix_object::Header {
                kind: *kind,
                size: data.len() as u64,
            }));
        }
        let Some(odb) = &self.odb else {
            return Ok(None);
        };
        let (size, kind) = match odb.read_header(git2_oid(id)) {
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

impl gix_object::Exists for StagingOdb<'_> {
    fn exists(&self, id: &gix_hash::oid) -> bool {
        self.pending.contains_key(id)
            || self
                .odb
                .as_ref()
                .is_some_and(|odb| odb.exists(git2_oid(id)))
    }
}
