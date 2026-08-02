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

pub mod revwalk;

pub use revwalk::{RangeWalk, RevWalk};

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

/// Whether `name` is acceptable as a tree entry name, replicating libgit2's `valid_entry_name`
/// under default configuration. gix by design performs no name validation when writing trees, so
/// josh owns this check; tree rewriting drops entries with invalid names (and marks the tree as
/// changed), exactly like the failed `git_treebuilder_insert` did before the gix port.
///
/// Rejected are: the empty name, names containing `/`, `.` and `..`, and `.git` including its
/// NTFS-style aliases (`.git.`, `.git `, `.git:x`, `.git\x`, case-insensitive) and HFS-style
/// aliases (`.git` with HFS-ignorable code points interspersed). libgit2 applies the NTFS check
/// everywhere (`core.protectNTFS` defaults to true) but the HFS check only on Apple platforms;
/// josh applies both everywhere so filtered output does not depend on the host OS -- for the
/// HFS-alias corner case on non-Apple hosts this is deliberately stricter than libgit2 was.
pub fn tree_entry_name_valid(name: &[u8]) -> bool {
    if name.is_empty() || name == b"." || name == b".." {
        return false;
    }
    if name.contains(&b'/') {
        return false;
    }
    // NTFS protection: a name starting with ".git" (case-insensitive) is invalid if what follows
    // could still address the ".git" directory on Windows: a `\` path separator, a `:` alternate
    // data stream, or nothing but trailing spaces and dots (which NTFS strips).
    if name.len() >= 4 && name[..4].eq_ignore_ascii_case(b".git") {
        let rest = &name[4..];
        if matches!(rest.first(), Some(b'\\') | Some(b':'))
            || rest.iter().all(|&c| c == b' ' || c == b'.')
        {
            return false;
        }
    }
    // HFS protection: the name must not read as ".git" (case-insensitive) after removing the
    // code points HFS+ ignores in filenames.
    if hfs_reads_as_dotgit(name) {
        return false;
    }
    true
}

/// True if `name` equals ".git" case-insensitively once HFS-ignorable code points are removed.
/// Mirrors libgit2's `validate_dotgit_hfs`. Non-UTF-8 names cannot alias ".git" this way.
fn hfs_reads_as_dotgit(name: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(name) else {
        return false;
    };
    let mut chars = s.chars().filter(|c| {
        !matches!(c,
            '\u{200c}' | '\u{200d}' | '\u{200e}' | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{206a}'..='\u{206f}'
            | '\u{feff}')
    });
    let mut expected = ".git".chars();
    loop {
        match (chars.next(), expected.next()) {
            (None, None) => return true,
            (Some(c), Some(e)) if c.to_ascii_lowercase() == e => {}
            _ => return false,
        }
    }
}

/// Normalize a raw tree entry mode exactly like libgit2's `normalize_filemode` (which every
/// `git_treebuilder_insert` applied): trees stay trees, any exec bit makes an executable blob,
/// gitlinks and symlinks keep their type, everything else becomes a plain blob. Note the exec
/// check precedes the gitlink/symlink checks and tests all three exec bits -- both quirks are
/// libgit2's, kept for byte-identical rebuilt trees.
pub fn normalize_filemode(raw: u16) -> u16 {
    if raw & 0o170000 == 0o040000 {
        0o040000
    } else if raw & 0o111 != 0 {
        0o100755
    } else if raw & 0o170000 == 0o160000 {
        0o160000
    } else if raw & 0o170000 == 0o120000 {
        0o120000
    } else {
        0o100644
    }
}

/// Serialize `entries` as a git tree in canonical order and write it straight to `odb`. Used on
/// hot paths while git2 still owns all I/O: the write is immediately visible to every reader of
/// the repository (and lands in the in-memory memodb store when one is registered), so ported and
/// unported code can interleave freely.
///
/// TODO(byte-preservation): the unconditional sort is part of the libgit2-parity normalization
/// of fsck-invalid input trees described on `TreeRebuild` in josh-core's filter/tree.rs. Once
/// filters preserve non-canonical trees byte-for-byte, rebuilds seeded from such trees will need
/// an order-preserving variant of this function (callers building entry sets from scratch keep
/// the sort).
pub fn write_tree_now(
    odb: &git2::Odb,
    mut entries: Vec<gix_object::tree::Entry>,
) -> anyhow::Result<git2::Oid> {
    entries.sort();
    let tree = gix_object::Tree { entries };
    let mut buffer = Vec::with_capacity(tree.size() as usize);
    tree.write_to(&mut buffer)?;
    Ok(odb.write(git2::ObjectType::Tree, &buffer)?)
}

/// A commit's id + raw odb bytes, owned. Send + Sync plain data; never stored in a transaction.
/// Parse-on-demand: `CommitRef`/`CommitRefIter` borrow `&self` and never outlive the frame.
/// The internal representation can become `Arc<[u8]>` later without any signature change.
#[derive(Clone, Debug)]
pub struct CommitData {
    id: git2::Oid,
    bytes: Vec<u8>,
}

impl CommitData {
    /// odb.read + kind check. Errors if the object is missing or not a commit -- matching
    /// `find_commit`'s contract (both are hard errors there too). Reads go through the odb's
    /// registered backends (memodb visibility preserved).
    pub fn read(odb: &git2::Odb<'_>, oid: git2::Oid) -> anyhow::Result<CommitData> {
        let obj = odb.read(oid)?;
        if obj.kind() != git2::ObjectType::Commit {
            return Err(anyhow::anyhow!(
                "object {} is not a commit but a {:?}",
                oid,
                obj.kind()
            ));
        }
        Ok(CommitData {
            id: oid,
            bytes: obj.data().to_vec(),
        })
    }

    pub fn id(&self) -> git2::Oid {
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

    pub fn tree_id(&self) -> anyhow::Result<git2::Oid> {
        let id =
            gix_object::CommitRefIter::from_bytes(&self.bytes, gix_hash::Kind::Sha1).tree_id()?;
        Ok(git2_oid(&id))
    }

    /// Binary parent ids via `CommitRefIter::parent_ids`. Identical to git2's `ParentIds`,
    /// which reads the parsed id array and never does odb lookups.
    pub fn parent_ids(&self) -> impl Iterator<Item = git2::Oid> + '_ {
        gix_object::CommitRefIter::from_bytes(&self.bytes, gix_hash::Kind::Sha1)
            .parent_ids()
            .map(|p| git2_oid(&p))
    }

    pub fn first_parent_id(&self) -> Option<git2::Oid> {
        self.parent_ids().next()
    }

    pub fn parent_count(&self) -> usize {
        self.parent_ids().count()
    }
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
                object_hash: id.kind(),
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
        Ok(Some(gix_object::Data {
            kind,
            object_hash: id.kind(),
            data: buffer,
        }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_name_validation() {
        for valid in [
            &b"foo"[..],
            b"git",
            b".gitignore",
            b".github",
            b".git0",
            b"a.git",
            b"..a",
            b"...",
            b".git.a",
        ] {
            assert!(tree_entry_name_valid(valid), "{valid:?} should be valid");
        }
        for invalid in [
            &b""[..],
            b".",
            b"..",
            b"a/b",
            b".git",
            b".GIT",
            b".GiT",
            b".git.",
            b".git ",
            b".git . .",
            b".git\\evil",
            b".GIT:stream",
            ".g\u{200c}it".as_bytes(),
            ".\u{feff}GIT".as_bytes(),
        ] {
            assert!(
                !tree_entry_name_valid(invalid),
                "{invalid:?} should be invalid"
            );
        }
    }

    #[test]
    fn filemode_normalization() {
        for (raw, norm) in [
            (0o100644, 0o100644),
            (0o100664, 0o100644),
            (0o100600, 0o100644),
            (0o100755, 0o100755),
            (0o100775, 0o100755),
            (0o100700, 0o100755),
            (0o100010, 0o100755),
            (0o040000, 0o040000),
            (0o040755, 0o040000),
            (0o120000, 0o120000),
            (0o160000, 0o160000),
            // libgit2 quirks: exec bits win over the gitlink/symlink type checks.
            (0o120755, 0o100755),
            (0o160755, 0o100755),
            // Garbage type bits collapse to a plain blob.
            (0o110644, 0o100644),
        ] {
            assert_eq!(normalize_filemode(raw), norm, "raw mode {raw:o}");
        }
    }
}
