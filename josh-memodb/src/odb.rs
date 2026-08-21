//! The transaction object-database facade: the [`MemOdb`] store consulted directly, backed by
//! the repository's git2 object database for objects that are not buffered.
//!
//! Implements the [`gix_object`] object-access traits (memory-first, disk fallback) plus
//! inherent helpers in `git2::Oid` currency; memory hits hand out zero-copy `Arc` buffers.
//! The `disk` side is `repo.odb()`, including any runtime alternates added to that repository
//! handle (josh-proxy overlays add the mirror), so the facade and plain git2 calls resolve
//! exactly the same objects. Write dedup goes through the caller's [`Alternates`] mirror
//! instead (see [`Odb::write`]) and never touches `disk`.

use std::sync::{Arc, Mutex};

use gix_hash::ObjectId;
use gix_object::Kind;

use crate::MemOdb;

/// A mirror of the runtime alternates registered on a repository handle: an odb over ONLY the
/// alternate object directories, no repository attached. The facade's write path probes it so
/// alternate-resident objects (the proxy overlay's mirror) are never buffered — flushed packs
/// must not contain duplicates of mirror objects, and the pack-time disk filter cannot see
/// runtime alternates. Deliberately excludes the repository's own odb: locally-durable objects
/// are dropped at pack time instead, and with no alternates registered the probe is a `None`
/// check, no filesystem I/O.
///
/// Owned by the holder of the repository handle rather than by the store, because what an
/// alternate makes reachable is a property of the handle: josh-templates registers the overlay
/// as an alternate of a mirror repository and the reverse elsewhere, and a store shared by both
/// must not mix them.
#[derive(Default)]
pub struct Alternates(Mutex<Option<git2::Odb<'static>>>);

impl Alternates {
    /// Record `path` (an objects directory) as a runtime alternate. Callers add the alternate to
    /// their repository odb themselves (`git2::Odb::add_disk_alternate`); this mirror only feeds
    /// the write-dedup probe.
    pub fn add(&self, path: &str) -> Result<(), git2::Error> {
        let mut alternates = self.0.lock().unwrap();
        let odb = match alternates.as_mut() {
            Some(odb) => odb,
            None => alternates.insert(git2::Odb::new()?),
        };
        odb.add_disk_alternate(path)
    }

    /// Whether `oid` exists in one of the recorded alternates. `false` without filesystem I/O
    /// when none are recorded.
    fn contains(&self, oid: git2::Oid) -> bool {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|o| o.exists(oid))
    }
}

/// Raw object bytes from a facade read: zero-copy shared buffer for memory hits, a libgit2
/// odb object for disk hits. Derefs to the raw serialized object bytes either way.
pub enum Bytes<'a> {
    Mem(Arc<[u8]>),
    Disk(git2::OdbObject<'a>),
}

impl std::ops::Deref for Bytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Mem(data) => data,
            Bytes::Disk(obj) => obj.data(),
        }
    }
}

/// See the module docs. A cheap per-call value: an `Arc` clone onto the transaction's store
/// plus the repository's odb handle, obtained via `Transaction::odb()`.
pub struct Odb<'repo> {
    mem: Arc<MemOdb>,
    disk: git2::Odb<'repo>,
    alternates: Option<&'repo Alternates>,
}

impl<'repo> Odb<'repo> {
    /// A facade with no runtime alternates registered, for repositories that have none.
    pub fn new(mem: Arc<MemOdb>, disk: git2::Odb<'repo>) -> Self {
        Odb {
            mem,
            disk,
            alternates: None,
        }
    }

    /// A facade whose write gate skips objects reachable through `alternates` (see there).
    pub fn with_alternates(
        mem: Arc<MemOdb>,
        disk: git2::Odb<'repo>,
        alternates: &'repo Alternates,
    ) -> Self {
        Odb {
            mem,
            disk,
            alternates: Some(alternates),
        }
    }

    /// Read the raw bytes and kind of `oid`; memory hits are zero-copy. A missing object is an
    /// error, like a plain odb read.
    pub fn read(&self, oid: git2::Oid) -> Result<(Kind, Bytes<'_>), git2::Error> {
        if let Some((kind, data)) = self.mem.get(&josh_gix_ext::gix_oid(oid)) {
            return Ok((kind, Bytes::Mem(data)));
        }
        let obj = self.disk.read(oid)?;
        let kind = josh_gix_ext::gix_kind(obj.kind()).ok_or_else(|| {
            git2::Error::new(
                git2::ErrorCode::Invalid,
                git2::ErrorClass::Odb,
                "object has no concrete kind",
            )
        })?;
        Ok((kind, Bytes::Disk(obj)))
    }

    /// Kind and size of `oid` without reading (or decompressing) its bytes.
    pub fn read_header(&self, oid: git2::Oid) -> Result<(Kind, u64), git2::Error> {
        if let Some(header) = self.mem.header(&josh_gix_ext::gix_oid(oid)) {
            return Ok(header);
        }
        let (size, kind) = self.disk.read_header(oid)?;
        let kind = josh_gix_ext::gix_kind(kind).ok_or_else(|| {
            git2::Error::new(
                git2::ErrorCode::Invalid,
                git2::ErrorClass::Odb,
                "object has no concrete kind",
            )
        })?;
        Ok((kind, size as u64))
    }

    pub fn contains(&self, oid: git2::Oid) -> bool {
        self.mem.contains(&josh_gix_ext::gix_oid(oid)) || self.disk.exists(oid)
    }

    /// Kind of `oid`, or `None` if the object does not exist. Never decompresses. The disk
    /// fallback resolves the empty tree even when it is stored nowhere, so probes on
    /// possibly-empty trees belong here and never on [`contains`](Odb::contains).
    pub fn try_kind(&self, oid: git2::Oid) -> Result<Option<Kind>, git2::Error> {
        if let Some((kind, _)) = self.mem.header(&josh_gix_ext::gix_oid(oid)) {
            return Ok(Some(kind));
        }
        let (_, kind) = match self.disk.read_header(oid) {
            Ok(h) => h,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        Ok(josh_gix_ext::gix_kind(kind))
    }

    /// Hash and buffer an object in the memory store, returning its content id. The buffer is
    /// skipped when the object is already in memory or in a runtime alternate: proxy overlays
    /// must never buffer mirror objects, or flushed packs would gain duplicates and change
    /// names (the pack-time disk filter cannot see runtime alternates). The gate never probes
    /// the repository's own on-disk odb: objects already durable there are buffered anyway and
    /// dropped at pack time, and a repository without alternates writes with zero filesystem
    /// I/O.
    pub fn write(&self, kind: Kind, data: &[u8]) -> git2::Oid {
        let id = gix_object::compute_hash(gix_hash::Kind::Sha1, kind, data)
            .expect("failed to compute hash");
        self.write_with_id(id, kind, data);
        josh_gix_ext::git2_oid(&id)
    }

    /// [`Odb::write`] with a caller-computed content hash, trusted verbatim.
    fn write_with_id(&self, id: ObjectId, kind: Kind, data: &[u8]) {
        if self.mem.contains(&id)
            || self
                .alternates
                .is_some_and(|alts| alts.contains(josh_gix_ext::git2_oid(&id)))
        {
            return;
        }
        self.mem.write_with_id(id, kind, data);
    }
}

impl gix_object::Find for Odb<'_> {
    fn try_find<'b>(
        &self,
        id: &gix_hash::oid,
        buffer: &'b mut Vec<u8>,
    ) -> Result<Option<gix_object::Data<'b>>, gix_object::find::Error> {
        if let Some((kind, data)) = self.mem.get(id) {
            buffer.clear();
            buffer.extend_from_slice(&data);
            return Ok(Some(gix_object::Data {
                kind,
                object_hash: id.kind(),
                data: buffer,
            }));
        }
        let obj = match self.disk.read(josh_gix_ext::git2_oid(id)) {
            Ok(obj) => obj,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(Box::new(e)),
        };
        let Some(kind) = josh_gix_ext::gix_kind(obj.kind()) else {
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

impl gix_object::FindHeader for Odb<'_> {
    fn try_header(
        &self,
        id: &gix_hash::oid,
    ) -> Result<Option<gix_object::Header>, gix_object::find::Error> {
        if let Some((kind, size)) = self.mem.header(id) {
            return Ok(Some(gix_object::Header { kind, size }));
        }
        let (size, kind) = match self.disk.read_header(josh_gix_ext::git2_oid(id)) {
            Ok(h) => h,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(Box::new(e)),
        };
        let Some(kind) = josh_gix_ext::gix_kind(kind) else {
            return Ok(None);
        };
        Ok(Some(gix_object::Header {
            kind,
            size: size as u64,
        }))
    }
}

impl gix_object::Exists for Odb<'_> {
    fn exists(&self, id: &gix_hash::oid) -> bool {
        self.mem.contains(id) || self.disk.exists(josh_gix_ext::git2_oid(id))
    }
}

impl gix_object::Write for Odb<'_> {
    fn write_buf(&self, object: Kind, from: &[u8]) -> Result<ObjectId, gix_object::write::Error> {
        Ok(josh_gix_ext::gix_oid(self.write(object, from)))
    }

    fn write_buf_with_known_id(
        &self,
        object: Kind,
        from: &[u8],
        id: ObjectId,
    ) -> Result<ObjectId, gix_object::write::Error> {
        self.write_with_id(id, object, from);
        Ok(id)
    }

    fn write_stream(
        &self,
        kind: Kind,
        size: u64,
        from: &mut dyn std::io::Read,
    ) -> Result<ObjectId, gix_object::write::Error> {
        let mut data = Vec::with_capacity(size as usize);
        from.read_to_end(&mut data)?;
        self.write_buf(kind, &data)
    }

    fn write_stream_with_known_id(
        &self,
        kind: Kind,
        size: u64,
        from: &mut dyn std::io::Read,
        id: ObjectId,
    ) -> Result<ObjectId, gix_object::write::Error> {
        let mut data = Vec::with_capacity(size as usize);
        from.read_to_end(&mut data)?;
        self.write_buf_with_known_id(kind, &data, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gix_object::{Exists as _, Find as _};

    fn facade<'a>(store: &Arc<MemOdb>, repo: &'a git2::Repository) -> Odb<'a> {
        Odb::new(store.clone(), repo.odb().unwrap())
    }

    /// A facade write is served from memory (zero-copy), is invisible to a plain reopened
    /// repository until `flush`, and durable after.
    #[test]
    fn facade_write_visible_before_flush_durable_after() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));

        let odb = facade(&store, &repo);
        let oid = odb.write(Kind::Blob, b"facade blob");

        let (kind, bytes) = odb.read(oid).unwrap();
        assert_eq!(kind, Kind::Blob);
        assert!(matches!(bytes, Bytes::Mem(_)));
        assert_eq!(&*bytes, b"facade blob");
        assert!(odb.contains(oid));

        // Not yet on disk.
        let fresh = git2::Repository::open(dir.path()).unwrap();
        assert!(fresh.find_blob(oid).is_err());

        store.flush().unwrap();
        let fresh = git2::Repository::open(dir.path()).unwrap();
        assert_eq!(fresh.find_blob(oid).unwrap().content(), b"facade blob");
    }

    /// The write gate: an object already in a registered runtime alternate is not buffered
    /// (so flushed packs never gain alternate duplicates), while an object merely on the
    /// repository's own disk is buffered and the pack-time filter drops it again.
    #[test]
    fn facade_write_gate_matches_freshen() {
        let tmp = tempfile::tempdir().unwrap();
        // "Mirror" repo holding the alternate-resident object.
        let mirror = git2::Repository::init(tmp.path().join("mirror")).unwrap();
        let in_alternate = mirror.blob(b"mirror blob").unwrap();

        let dir = tmp.path().join("overlay");
        let repo = git2::Repository::init(&dir).unwrap();
        // A loose blob, so it is main-disk-only.
        let on_disk = repo.blob(b"already on disk").unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let alt = crate::pack::objects_dir(&mirror);
        repo.odb()
            .unwrap()
            .add_disk_alternate(alt.to_str().unwrap())
            .unwrap();
        let alternates = Alternates::default();
        alternates.add(alt.to_str().unwrap()).unwrap();

        let odb = Odb::with_alternates(store.clone(), repo.odb().unwrap(), &alternates);

        // Alternate hit: skipped, still readable through the disk chain.
        let oid = odb.write(Kind::Blob, b"mirror blob");
        assert_eq!(oid, in_alternate);
        assert!(!store.contains(&josh_gix_ext::gix_oid(oid)));
        assert!(matches!(odb.read(oid).unwrap().1, Bytes::Disk(_)));

        // Main-disk object: buffered — the gate does not probe the repository's own odb.
        let oid = odb.write(Kind::Blob, b"already on disk");
        assert_eq!(oid, on_disk);
        assert!(store.contains(&josh_gix_ext::gix_oid(oid)));
        assert!(matches!(odb.read(oid).unwrap().1, Bytes::Mem(_)));
    }

    /// The empty tree reads as an existing tree through `try_kind` even when it is absent
    /// from memory AND disk, while `contains` reports it absent -- kind probes on
    /// possibly-empty trees rely on exactly this.
    #[test]
    fn try_kind_virtualizes_empty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let odb = facade(&store, &repo);

        let empty_tree = git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap();
        assert!(!store.contains(&josh_gix_ext::gix_oid(empty_tree)));
        assert_eq!(odb.try_kind(empty_tree).unwrap(), Some(Kind::Tree));
        assert!(!odb.contains(empty_tree));

        // A genuinely absent object is None; a buffered object reports its memory kind.
        let absent = git2::Oid::from_str("0123456789012345678901234567890123456789").unwrap();
        assert_eq!(odb.try_kind(absent).unwrap(), None);
        let blob = odb.write(Kind::Blob, b"probe");
        assert_eq!(odb.try_kind(blob).unwrap(), Some(Kind::Blob));
    }

    /// Buffered objects resolve through the gix trait surface, which is how every reader in
    /// josh reaches them.
    #[test]
    fn buffered_objects_resolve_through_the_gix_traits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let odb = facade(&store, &repo);

        let oid = odb.write(Kind::Blob, b"facade blob");

        let mut buf = Vec::new();
        let data = odb
            .try_find(&josh_gix_ext::gix_oid(oid), &mut buf)
            .unwrap()
            .unwrap();
        assert_eq!(data.kind, Kind::Blob);
        assert_eq!(data.data, b"facade blob");
        assert!(odb.exists(&josh_gix_ext::gix_oid(oid)));
    }
}
