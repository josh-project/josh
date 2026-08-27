//! The transaction object-database facade: the [`MemOdb`] store consulted directly, backed by
//! the repository's gitoxide object database for objects that are not buffered.
//!
//! Implements the [`gix_object`] object-access traits and `ObjectId`-typed inherent helpers
//! (memory-first, disk fallback); memory hits hand out zero-copy `Arc` buffers.
//!
//! A store resolves `objects/info/alternates` when it opens. An alternate registered at runtime
//! (the proxy overlay's mirror) is a store of its own. Non-refreshing handles probe every known
//! store first; only a miss everywhere may rescan object directories for packs written later.
//!
//! The empty tree is resolved here whether or not it is stored, and before disk: josh reads it
//! constantly as the base of a rebuild, and a lookup that misses makes gitoxide re-read the
//! objects directory.
//!
//! One facade per transaction, built once, so its cache and the store's pack indices serve a
//! whole filter run.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use gix_features::threading::OwnShared;
use gix_hash::ObjectId;
use gix_object::{Exists, Find, FindHeader, Kind};

use crate::MemOdb;

/// Decompressed objects kept per facade, so re-reading one does not decode it again. Worth
/// ~10% of a cold filter run; a 64 MB budget measures the same, so the size is not the lever.
const OBJECT_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// Raw object bytes from a facade read: a zero-copy shared buffer for memory hits, the
/// decompressed bytes for disk hits. Derefs to the raw serialized object bytes either way.
pub enum Bytes {
    Mem(Arc<[u8]>),
    Disk(Vec<u8>),
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Mem(data) => data,
            Bytes::Disk(data) => data,
        }
    }
}

/// See the module docs. Built by `Transaction::odb()` and held for the transaction.
pub struct Odb {
    mem: Arc<MemOdb>,
    /// The repository's own objects. Handles taken from one store share its loaded pack
    /// indices, so every transaction on a repository pays for them once between them.
    disk: gix_odb::Handle,
    /// A non-refreshing reader for the same store. Proxy objects live either here or in an
    /// alternate; probing both snapshots before either refreshing avoids a directory rescan
    /// for every object held by the other store.
    disk_fast: gix_odb::Handle,
    /// Object directories registered after the repository was opened; see the module docs.
    alternates: RefCell<Vec<Alternate>>,
}

/// A runtime alternate with snapshot and refreshing readers plus a write-dedup gate.
struct Alternate {
    /// Refreshes on a miss, so an object that landed after registration is found.
    read: gix_odb::Handle,
    /// Reads the packs known when this handle last loaded its snapshot, without rescanning on a
    /// miss. Carries an object cache, unlike `gate`.
    fast: gix_odb::Handle,
    /// The write gate (see [`Odb::write`]), which misses for every object josh produces, so it
    /// must not pay the directory scan a refresh costs. It answers from what the store already
    /// knows; missing an object packed since costs a duplicate in a pack, never correctness.
    gate: gix_odb::Handle,
}

/// The empty tree, which josh expects to resolve in every repository.
fn empty_tree() -> ObjectId {
    ObjectId::empty_tree(gix_hash::Kind::Sha1)
}

/// A reading handle on `store`, cached for the length of a filter run.
///
/// No delta-base cache, deliberately: `gix_pack` caches a decoded object under its own pack
/// offset, and a filter run reads history oldest first, so what it caches is never a later
/// read's chain ancestor and the cache cannot hit -- it only adds bookkeeping, measured at 4-7%
/// on the cases it should have helped. Reading deltified packs in that order is what the object
/// path still pays for; caching the chain's base in `decode_entry` would fix it upstream, and
/// nothing here can, since the cache trait only sees what `decode_entry` puts in it.
fn handle(store: OwnShared<gix_odb::Store>) -> gix_odb::Handle {
    let mut handle = gix_odb::Cache::from(store.to_handle());
    handle.set_object_cache(|| {
        Box::new(gix_pack::cache::object::MemoryCappedHashmap::new(
            OBJECT_CACHE_BYTES,
        ))
    });
    // Default refresh mode: a lookup that misses re-reads the objects directory, which is how a
    // store learns about a pack josh, a spawned `git` or another process has written.
    handle
}

impl Odb {
    /// The facade over `mem` and the objects of the repository `store` belongs to. Take the
    /// store from a repository handle so its pack indices are shared between transactions;
    /// [`Odb::at`] opens one for a bare objects directory.
    pub fn new(mem: Arc<MemOdb>, store: OwnShared<gix_odb::Store>) -> Self {
        let mut disk_fast = handle(store.clone());
        disk_fast.refresh_never();
        Odb {
            mem,
            disk: handle(store),
            disk_fast,
            alternates: Default::default(),
        }
    }

    /// [`Odb::new`] over the objects directory `objects_dir`, opening a store of its own.
    pub fn at(mem: Arc<MemOdb>, objects_dir: &Path) -> std::io::Result<Self> {
        Ok(Odb::new(mem, gix_odb::at(objects_dir)?.store()))
    }

    /// Register `path` (an objects directory) as another place to read objects from, and one
    /// whose objects the write gate must not buffer.
    pub fn add_alternate(&self, path: &Path) -> std::io::Result<()> {
        let store = gix_odb::at(path)?.store();
        let mut fast = handle(store.clone());
        fast.refresh_never();
        let mut gate = gix_odb::Cache::from(store.to_handle());
        gate.refresh_never();
        self.alternates.borrow_mut().push(Alternate {
            read: handle(store),
            fast,
            gate,
        });
        Ok(())
    }

    /// Whether a registered alternate holds `id`, refreshing its store if necessary.
    fn in_alternate(&self, id: &gix_hash::oid) -> bool {
        self.alternates
            .borrow()
            .iter()
            .any(|alternate| alternate.read.exists(id))
    }

    /// Whether a registered alternate's current snapshot holds `id`.
    fn in_alternate_fast(&self, id: &gix_hash::oid) -> bool {
        self.alternates
            .borrow()
            .iter()
            .any(|alternate| alternate.fast.exists(id))
    }

    /// [`Odb::in_alternate`] through the gate handles (see [`Alternate::gate`]).
    fn in_alternate_gate(&self, id: &gix_hash::oid) -> bool {
        self.alternates
            .borrow()
            .iter()
            .any(|alt| alt.gate.exists(id))
    }

    /// Read the raw bytes and kind of `id`; memory hits are zero-copy. A missing object is an
    /// error, like a plain odb read.
    pub fn read(&self, id: ObjectId) -> anyhow::Result<(Kind, Bytes)> {
        if let Some((kind, data)) = self.mem.get(&id) {
            return Ok((kind, Bytes::Mem(data)));
        }
        if id == empty_tree() {
            return Ok((Kind::Tree, Bytes::Disk(Vec::new())));
        }
        let mut buffer = Vec::new();
        if let Some(kind) = self
            .find_on_disk(&id, &mut buffer)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {}", id, e))?
        {
            return Ok((kind, Bytes::Disk(buffer)));
        }
        Err(anyhow::anyhow!(
            "object not found - no match for id ({})",
            id
        ))
    }

    /// Read `id` into `buffer` from the repository or an alternate, reporting the kind it was
    /// stored with, or `None` when neither holds it. Probe non-refreshing snapshots first: in a
    /// proxy overlay, each source-object lookup misses the primary and each generated-object
    /// lookup misses the mirror. Refreshing either miss would rescan an object directory per
    /// object.
    fn find_on_disk(
        &self,
        id: &gix_hash::oid,
        buffer: &mut Vec<u8>,
    ) -> Result<Option<Kind>, gix_object::find::Error> {
        if let Some(data) = self.disk_fast.try_find(id, buffer)? {
            return Ok(Some(data.kind));
        }
        for alternate in self.alternates.borrow().iter() {
            if let Some(data) = alternate.fast.try_find(id, buffer)? {
                return Ok(Some(data.kind));
            }
        }
        if let Some(data) = self.disk.try_find(id, buffer)? {
            return Ok(Some(data.kind));
        }
        for alternate in self.alternates.borrow().iter() {
            if let Some(data) = alternate.read.try_find(id, buffer)? {
                return Ok(Some(data.kind));
            }
        }
        Ok(None)
    }

    /// Kind and size of `id` without reading (or decompressing) its bytes.
    pub fn read_header(&self, id: ObjectId) -> anyhow::Result<(Kind, u64)> {
        self.header(id)?
            .ok_or_else(|| anyhow::anyhow!("object not found - no match for id ({})", id))
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.mem.contains(&id)
            || self.disk_fast.exists(&id)
            || self.in_alternate_fast(&id)
            || self.disk.exists(&id)
            || self.in_alternate(&id)
    }

    /// Kind of `id`, or `None` if the object does not exist. Never decompresses. Resolves the
    /// empty tree even when it is stored nowhere, so probes on possibly-empty trees belong
    /// here and never on [`contains`](Odb::contains).
    pub fn try_kind(&self, id: ObjectId) -> anyhow::Result<Option<Kind>> {
        Ok(self.header(id)?.map(|(kind, _)| kind))
    }

    /// Kind and size of `id`, or `None` when no store holds it.
    fn header(&self, id: ObjectId) -> anyhow::Result<Option<(Kind, u64)>> {
        Ok(self
            .try_header(&id)
            .map_err(|e| anyhow::anyhow!("failed to read header of {}: {}", id, e))?
            .map(|header| (header.kind, header.size)))
    }

    /// Hash and buffer an object in the memory store, returning its content id. The buffer is
    /// skipped when the object is already in memory or in a runtime alternate: proxy overlays
    /// must never buffer mirror objects, or flushed packs would gain duplicates and change
    /// names (the pack-time disk filter cannot see runtime alternates). The gate never probes
    /// the repository's own on-disk objects: objects already durable there are buffered anyway
    /// and dropped at pack time, and a repository without alternates writes with zero
    /// filesystem I/O.
    pub fn write(&self, kind: Kind, data: &[u8]) -> ObjectId {
        let id = gix_object::compute_hash(gix_hash::Kind::Sha1, kind, data)
            .expect("failed to compute hash");
        self.write_with_id(id, kind, data);
        id
    }

    /// [`Odb::write`] with a caller-computed content hash, trusted verbatim.
    fn write_with_id(&self, id: ObjectId, kind: Kind, data: &[u8]) {
        if self.mem.contains(&id) || self.in_alternate_gate(&id) {
            return;
        }
        self.mem.write_with_id(id, kind, data);
    }
}

impl gix_object::Find for Odb {
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
        if id == empty_tree() {
            buffer.clear();
            return Ok(Some(gix_object::Data {
                kind: Kind::Tree,
                object_hash: id.kind(),
                data: buffer,
            }));
        }
        let Some(kind) = self.find_on_disk(id, buffer)? else {
            return Ok(None);
        };
        Ok(Some(gix_object::Data {
            kind,
            object_hash: id.kind(),
            data: buffer,
        }))
    }
}

impl gix_object::FindHeader for Odb {
    fn try_header(
        &self,
        id: &gix_hash::oid,
    ) -> Result<Option<gix_object::Header>, gix_object::find::Error> {
        if let Some((kind, size)) = self.mem.header(id) {
            return Ok(Some(gix_object::Header { kind, size }));
        }
        if id == empty_tree() {
            return Ok(Some(gix_object::Header {
                kind: Kind::Tree,
                size: 0,
            }));
        }
        if let Some(header) = self.disk_fast.try_header(id)? {
            return Ok(Some(header));
        }
        for alternate in self.alternates.borrow().iter() {
            if let Some(header) = alternate.fast.try_header(id)? {
                return Ok(Some(header));
            }
        }
        if let Some(header) = self.disk.try_header(id)? {
            return Ok(Some(header));
        }
        for alternate in self.alternates.borrow().iter() {
            if let Some(header) = alternate.read.try_header(id)? {
                return Ok(Some(header));
            }
        }
        Ok(None)
    }
}

impl gix_object::Exists for Odb {
    fn exists(&self, id: &gix_hash::oid) -> bool {
        self.mem.contains(id)
            || self.disk_fast.exists(id)
            || self.in_alternate_fast(id)
            || self.disk.exists(id)
            || self.in_alternate(id)
    }
}

impl gix_object::Write for Odb {
    fn write_buf(&self, object: Kind, from: &[u8]) -> Result<ObjectId, gix_object::write::Error> {
        Ok(self.write(object, from))
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

    fn facade(store: &Arc<MemOdb>, repo: &gix::Repository) -> Odb {
        Odb::at(store.clone(), &crate::pack::objects_dir(repo)).unwrap()
    }

    /// A facade write is served from memory (zero-copy), is invisible to a plain reopened
    /// repository until `flush`, and durable after.
    #[test]
    fn facade_write_visible_before_flush_durable_after() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));

        let odb = facade(&store, &repo);
        let oid = odb.write(Kind::Blob, b"facade blob");

        let (kind, bytes) = odb.read(oid).unwrap();
        assert_eq!(kind, Kind::Blob);
        assert!(matches!(bytes, Bytes::Mem(_)));
        assert_eq!(&*bytes, b"facade blob");
        assert!(odb.contains(oid));

        // Not yet on disk.
        assert!(disk_blob(dir.path(), oid).is_none());

        store.flush().unwrap();
        assert_eq!(disk_blob(dir.path(), oid).unwrap(), b"facade blob");
    }

    /// The write gate: an object already in a registered runtime alternate is not buffered
    /// (so flushed packs never gain alternate duplicates), while an object merely on the
    /// repository's own disk is buffered and the pack-time filter drops it again.
    #[test]
    fn facade_write_gate_matches_freshen() {
        let tmp = tempfile::tempdir().unwrap();
        // "Mirror" repo holding the alternate-resident object.
        let mirror = gix::init(tmp.path().join("mirror")).unwrap();
        let in_alternate = write_disk_blob(&mirror, b"mirror blob");

        let dir = tmp.path().join("overlay");
        let repo = gix::init(&dir).unwrap();
        // A loose blob, so it is main-disk-only.
        let on_disk = write_disk_blob(&repo, b"already on disk");
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));

        let odb = facade(&store, &repo);
        odb.add_alternate(&crate::pack::objects_dir(&mirror))
            .unwrap();

        // Alternate hit: skipped, still readable through the alternate.
        let oid = odb.write(Kind::Blob, b"mirror blob");
        assert_eq!(oid, in_alternate);
        assert!(!store.contains(&oid));
        assert!(matches!(odb.read(oid).unwrap().1, Bytes::Disk(_)));

        // Main-disk object: buffered — the gate does not probe the repository's own objects.
        let oid = odb.write(Kind::Blob, b"already on disk");
        assert_eq!(oid, on_disk);
        assert!(store.contains(&oid));
        assert!(matches!(odb.read(oid).unwrap().1, Bytes::Mem(_)));
    }

    /// The gate sees a packed alternate object, which is the proxy's arrangement. One packed
    /// after registration is deliberately outside what it sees, while reads still find it.
    #[test]
    fn facade_write_gate_sees_packed_alternates() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = gix::init(tmp.path().join("mirror")).unwrap();
        let mirror_dir = crate::pack::objects_dir(&mirror);
        // Packed before the alternate is registered.
        let mirror_store = MemOdb::new(None, mirror_dir.clone());
        let packed = mirror_store.write(Kind::Blob, b"packed in mirror");
        mirror_store.flush().unwrap();

        let repo = gix::init(tmp.path().join("overlay")).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let odb = facade(&store, &repo);
        odb.add_alternate(&mirror_dir).unwrap();

        assert_eq!(odb.write(Kind::Blob, b"packed in mirror"), packed);
        assert!(!store.contains(&packed));

        // Packed after registration: the gate does not go looking, so it buffers a duplicate.
        let later = mirror_store.write(Kind::Blob, b"packed later");
        let unread = mirror_store.write(Kind::Blob, b"never written");
        mirror_store.flush().unwrap();
        assert_eq!(odb.write(Kind::Blob, b"packed later"), later);
        assert!(store.contains(&later));

        // Reads do go looking, and find an object from that same pack.
        assert_eq!(&*odb.read(unread).unwrap().1, b"never written");
    }

    /// The empty tree reads as an existing tree through `try_kind` even when it is absent
    /// from memory AND disk, while `contains` reports it absent -- kind probes on
    /// possibly-empty trees rely on exactly this.
    #[test]
    fn try_kind_virtualizes_empty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let odb = facade(&store, &repo);

        let empty_tree = ObjectId::from_hex(b"4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap();
        assert!(!store.contains(&empty_tree));
        assert_eq!(odb.try_kind(empty_tree).unwrap(), Some(Kind::Tree));
        assert_eq!(odb.read(empty_tree).unwrap().0, Kind::Tree);
        assert!(odb.read(empty_tree).unwrap().1.is_empty());
        assert!(!odb.contains(empty_tree));

        // A genuinely absent object is None; a buffered object reports its memory kind.
        let absent = ObjectId::from_hex(b"0123456789012345678901234567890123456789").unwrap();
        assert_eq!(odb.try_kind(absent).unwrap(), None);
        assert!(odb.read(absent).is_err());
        let blob = odb.write(Kind::Blob, b"probe");
        assert_eq!(odb.try_kind(blob).unwrap(), Some(Kind::Blob));
    }

    /// Buffered objects resolve through the gix trait surface, which is how every reader in
    /// josh reaches them.
    #[test]
    fn buffered_objects_resolve_through_the_gix_traits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let odb = facade(&store, &repo);

        let oid = odb.write(Kind::Blob, b"facade blob");

        let mut buf = Vec::new();
        let data = odb.try_find(&oid, &mut buf).unwrap().unwrap();
        assert_eq!(data.kind, Kind::Blob);
        assert_eq!(data.data, b"facade blob");
        assert!(odb.exists(&oid));
    }

    /// A pack written after the facade was built is still found, which is how a flush, a
    /// spawned `git` and other processes stay visible to a store that outlives them.
    #[test]
    fn objects_packed_after_the_facade_was_built_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let odb = facade(&store, &repo);

        // Miss first, so the store has settled on the packs it found at open.
        let oid =
            gix_object::compute_hash(gix_hash::Kind::Sha1, Kind::Blob, b"packed later").unwrap();
        assert!(!odb.contains(oid));

        let store2 = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let facade2 = facade(&store2, &repo);
        assert_eq!(facade2.write(Kind::Blob, b"packed later"), oid);
        store2.flush().unwrap();

        assert!(odb.contains(oid));
        assert_eq!(&*odb.read(oid).unwrap().1, b"packed later");
    }

    fn write_disk_blob(repo: &gix::Repository, data: &[u8]) -> ObjectId {
        gix_object::Write::write_buf(&repo.objects, Kind::Blob, data).unwrap()
    }

    fn disk_blob(repo_path: &std::path::Path, id: ObjectId) -> Option<Vec<u8>> {
        let repo = gix::open(repo_path).ok()?;
        let mut buf = Vec::new();
        let data = gix_object::Find::try_find(&repo.objects, &id, &mut buf)
            .ok()??
            .data;
        Some(data.to_vec())
    }
}
