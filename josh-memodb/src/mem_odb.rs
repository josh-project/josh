//! A per-operation in-memory ODB store buffering the objects josh produces while filtering, instead
//! of writing each as a loose object. Packed to a packfile at transaction and external-git
//! boundaries, and additionally packed mid-transaction once the buffered object data exceeds the
//! configured size limit (so a single transaction may write several packfiles). One [`MemOdb`] per
//! operation (not process-global), so flushes are isolated and deterministic.
//!
//! Packing itself runs on a background thread (see [`crate::flusher`]): the write path enqueues the
//! work and keeps filtering, and a boundary flush blocks only until the pack is durable. The store's
//! `Mutex` therefore guards concurrent access from the filter thread (writes/reads through the
//! backend) and the flusher thread (which snapshots the buffered objects to pack them, then evicts
//! them); it is `Send + Sync` so its `Arc` can cross to the flusher.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use git2::{ErrorClass, ErrorCode, ObjectType, Oid};
use gix_hash::ObjectId;
use gix_object::Kind;

use crate::odb_backend::{self, OdbBackend};

/// Object bytes are held behind an `Arc` so a flush can snapshot them without copying: the store
/// must keep serving reads while the flusher packs (eviction only happens once the pack is
/// durable), so the snapshot shares the buffers instead of draining or cloning them.
type ObjectMap = BTreeMap<ObjectId, (Kind, Arc<[u8]>)>;

/// A `Send` snapshot of buffered objects in sorted-oid order (the `ObjectMap` iteration order),
/// which keeps the resulting packfile — and hence its checksum-derived name — deterministic.
pub(crate) type Snapshot = Vec<(ObjectId, Kind, Arc<[u8]>)>;

/// The buffered objects plus a running total of their data size, guarded by one lock so that an
/// insert and the overflow check following it are observed atomically.
struct Inner {
    map: ObjectMap,
    size: usize,
}

/// A per-operation in-memory object store. Create one with [`MemOdb::new`], attach it to a
/// repository's object database with [`MemOdb::register`], and drain it to disk with
/// [`MemOdb::flush`]. When `limit` is set, the store also enqueues a background pack from the write
/// path as soon as the buffered data exceeds it (see [`MemOdb::enqueue_chunk`]).
pub struct MemOdb {
    inner: Mutex<Inner>,
    limit: Option<usize>,
    /// The registered repository's resolved object directory (`<commondir>/objects`, see
    /// [`crate::pack::objects_dir`]), captured at construction while the caller holds a repository
    /// handle; the background flusher packs straight into it without opening one.
    objects_dir: PathBuf,
    /// Set while an overflow chunk is queued on or running in the background flusher, so the write
    /// path does not pile up redundant chunk requests every time the store crosses its limit.
    /// Cleared by the flusher once the chunk has packed and evicted.
    chunk_in_flight: AtomicBool,
    /// A mirror of the runtime alternates registered on the owning repository
    /// (see [`MemOdb::add_alternate`]): an odb over ONLY the alternate object directories, no
    /// repository attached. The direct write path probes it so alternate-resident objects
    /// (the proxy overlay's mirror) are never buffered — flushed packs must not contain
    /// duplicates of mirror objects, and the pack-time disk filter cannot see runtime
    /// alternates. Deliberately excludes the repository's own odb: locally-durable objects
    /// are dropped at pack time instead, and with no alternates registered the probe is a
    /// `None` check, no filesystem I/O.
    alternates: Mutex<Option<git2::Odb<'static>>>,
}

impl MemOdb {
    /// Create an empty store. Returned as an [`Arc`] because the store is shared between the owning
    /// transaction and the libgit2 backend registered on its repository. `limit` bounds the total
    /// buffered object data: once exceeded the store flushes itself to a packfile (`None` = unbounded).
    /// `objects_dir` is where flushes land; pass [`crate::pack::objects_dir`] of the repository the
    /// store is about to be registered on.
    pub fn new(limit: Option<usize>, objects_dir: PathBuf) -> Arc<MemOdb> {
        Arc::new(MemOdb {
            inner: Mutex::new(Inner {
                map: Default::default(),
                size: 0,
            }),
            limit,
            objects_dir,
            chunk_in_flight: AtomicBool::new(false),
            alternates: Mutex::new(None),
        })
    }

    /// Record `path` (an objects directory) as a runtime alternate of the owning repository.
    /// Callers add the alternate to the repository odb themselves (`Odb::add_disk_alternate`);
    /// this mirror only feeds the direct write path's dedup probe (see [`Self::alternates`]).
    pub fn add_alternate(&self, path: &str) -> Result<(), git2::Error> {
        let mut alternates = self.alternates.lock().unwrap();
        let odb = match alternates.as_mut() {
            Some(odb) => odb,
            None => alternates.insert(git2::Odb::new()?),
        };
        odb.add_disk_alternate(path)
    }

    /// Whether `oid` exists in one of the registered alternates. `false` without filesystem
    /// I/O when no alternates are registered.
    pub(crate) fn exists_in_alternates(&self, oid: Oid) -> bool {
        self.alternates
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|odb| odb.exists(oid))
    }

    /// Replace `repo`'s object database with a backend that reads from / writes to this store,
    /// delegating read-side misses to the original on-disk ODB (see [`odb_backend::register`]), so
    /// writes land in memory and reads check memory first.
    pub fn register(self: &Arc<Self>, repo: &git2::Repository) {
        let _ = odb_backend::register(
            repo,
            MemBackend {
                store: self.clone(),
            },
        );
    }

    /// Buffer `oid` and return whether the store has now exceeded its size limit, so the caller
    /// can flush. A content-addressed duplicate is a no-op and never reports overflow: it adds
    /// no bytes, so it must not trigger a pack.
    fn insert(&self, oid: ObjectId, kind: Kind, data: Arc<[u8]>) -> bool {
        let len = data.len();
        let mut inner = self.inner.lock().unwrap();
        if inner.map.insert(oid, (kind, data)).is_some() {
            return false;
        }
        inner.size += len;
        self.limit.is_some_and(|limit| inner.size > limit)
    }

    /// Hash `data` and buffer it, enqueueing a background pack if the store overflows its
    /// limit. Callers that dedup against on-disk objects gate before calling (see
    /// [`crate::odb::Odb`]); the store itself only dedups against its own buffered contents.
    pub fn write(self: &Arc<Self>, kind: Kind, data: &[u8]) -> ObjectId {
        let id = gix_object::compute_hash(gix_hash::Kind::Sha1, kind, data)
            .expect("failed to compute hash");
        self.write_with_id(id, kind, data);
        id
    }

    /// [`MemOdb::write`] with a caller-computed content hash, trusted verbatim.
    pub fn write_with_id(self: &Arc<Self>, id: ObjectId, kind: Kind, data: &[u8]) {
        if self.insert(id, kind, data.into()) {
            self.enqueue_chunk();
        }
    }

    /// The buffered object `id`, as a zero-copy clone of the store's shared buffer.
    pub fn get(&self, id: &gix_hash::oid) -> Option<(Kind, Arc<[u8]>)> {
        self.inner
            .lock()
            .unwrap()
            .map
            .get(id)
            .map(|(kind, data)| (*kind, data.clone()))
    }

    /// Kind and size of the buffered object `id`, without touching its bytes.
    pub fn header(&self, id: &gix_hash::oid) -> Option<(Kind, u64)> {
        self.inner
            .lock()
            .unwrap()
            .map
            .get(id)
            .map(|(kind, data)| (*kind, data.len() as u64))
    }

    pub fn contains(&self, id: &gix_hash::oid) -> bool {
        self.inner.lock().unwrap().map.contains_key(id)
    }

    /// Drain every in-memory object into a packfile on disk and evict it from the store, blocking
    /// until done. Called when the owning transaction completes (or at an explicit external-git
    /// boundary) so the on-disk repository is left whole for any subsequent process that expects the
    /// objects on disk. Packing runs on the background flusher, behind any overflow chunks already
    /// queued for this store, so it returns only once every buffered object is durable.
    pub fn flush(self: &Arc<Self>) -> Result<(), git2::Error> {
        crate::flusher::drain(self.clone())
    }

    /// Start packing the store's current contents in the background without blocking. Best-effort:
    /// durability is only guaranteed by a subsequent [`MemOdb::flush`], which the FIFO flusher
    /// serializes behind any queued chunk. Use this when the caller wants packing to overlap its
    /// ongoing work and has a later drain point as the durability barrier.
    pub fn pack_in_background(self: &Arc<Self>) {
        self.enqueue_chunk();
    }

    /// Enqueue a best-effort background pack of this store, called from the ODB `write` path when the
    /// store overflows its size limit. `chunk_in_flight` collapses the burst of overflowing writes
    /// that follow into a single queued chunk; the flusher clears it once the chunk has packed and
    /// evicted (so `size` reflects the drain before another chunk can be enqueued).
    fn enqueue_chunk(self: &Arc<Self>) {
        if self.chunk_in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        crate::flusher::enqueue_chunk(self.clone());
    }

    /// Clear the [`Self::chunk_in_flight`] guard. Called by the background flusher after a chunk.
    pub(crate) fn clear_chunk_in_flight(&self) {
        self.chunk_in_flight.store(false, Ordering::Release);
    }

    /// Pack this store's currently-buffered objects into a packfile and evict them. Runs only on
    /// the background flusher, which packs a snapshot of the buffers straight into the object
    /// directory captured at construction (see [`crate::pack::write_snapshot`]) — no repository
    /// handle involved. The snapshot shares the object buffers (`Arc`), and the objects stay in the
    /// map while the pack is written, so concurrent reads through the backend keep resolving until
    /// eviction — which happens only once the pack is on disk.
    pub(crate) fn pack_to_disk(self: &Arc<Self>) -> Result<(), git2::Error> {
        // Snapshot under the lock, then release it: `write_snapshot` filters against the on-disk
        // store and compresses every object, which must not stall the filter thread's reads and
        // writes. Objects already on disk may have been re-buffered (the memory-only freshen does
        // not deduplicate against disk, see `odb_backend`); `write_snapshot` packs only the
        // genuinely-new ones, in the map's sorted oid order, keeping the packfile — and its
        // checksum-derived name — deterministic.
        let snapshot: Snapshot = {
            let inner = self.inner.lock().unwrap();
            if inner.map.is_empty() {
                return Ok(());
            }
            inner
                .map
                .iter()
                .map(|(oid, (kind, data))| (*oid, *kind, data.clone()))
                .collect()
        };

        crate::pack::write_snapshot(&self.objects_dir, &snapshot)?;

        // Evict exactly the snapshotted oids (now durable: packed just above, or already on disk).
        // Writes that landed after the snapshot stay buffered for the next chunk or the drain, so a
        // background chunk running concurrently with the write path never drops a live object.
        let mut inner = self.inner.lock().unwrap();
        for (oid, _, _) in &snapshot {
            if let Some((_, data)) = inner.map.remove(oid) {
                inner.size = inner.size.saturating_sub(data.len());
            }
        }
        Ok(())
    }
}

/// The libgit2 backend handle for a [`MemOdb`]: a cheap [`Arc`] onto the store, registered on one
/// repository by [`MemOdb::register`].
struct MemBackend {
    store: Arc<MemOdb>,
}

fn not_found() -> git2::Error {
    // ErrorCode::NotFound signals a memory miss, which the backend trampolines resolve by reading
    // through to the delegate on-disk ODB instead of treating it as a hard failure.
    git2::Error::new(
        ErrorCode::NotFound,
        ErrorClass::Odb,
        "not in in-memory store",
    )
}

impl OdbBackend for MemBackend {
    fn read_header(&self, oid: Oid) -> Result<(usize, ObjectType), git2::Error> {
        self.store
            .header(&josh_gix_ext::gix_oid(oid))
            .map(|(kind, size)| (size as usize, josh_gix_ext::git2_kind(kind)))
            .ok_or_else(not_found)
    }

    fn read(&self, oid: Oid) -> Result<(Vec<u8>, ObjectType), git2::Error> {
        self.store
            .get(&josh_gix_ext::gix_oid(oid))
            .map(|(kind, data)| (data.to_vec(), josh_gix_ext::git2_kind(kind)))
            .ok_or_else(not_found)
    }

    fn write(&mut self, oid: Oid, data: Vec<u8>, kind: ObjectType) -> Result<(), git2::Error> {
        // No dedup gate here: `git_odb__freshen` runs before this trampoline and is the
        // dedup for the git2 write path.
        let Some(kind) = josh_gix_ext::gix_kind(kind) else {
            return Err(git2::Error::new(
                git2::ErrorCode::Invalid,
                ErrorClass::Odb,
                "not a writable object kind",
            ));
        };
        let overflow = self
            .store
            .insert(josh_gix_ext::gix_oid(oid), kind, data.into());
        if overflow {
            self.store.enqueue_chunk();
        }
        Ok(())
    }

    fn exists(&self, oid: Oid) -> bool {
        self.store.contains(&josh_gix_ext::gix_oid(oid))
    }

    fn exists_prefix(&self, _oid: Oid, _oid_len: usize) -> Option<Oid> {
        // Abbreviated-OID lookup against the in-memory store is not implemented; full-OID lookups
        // (the hot path) are unaffected, and prefix lookups fall through to the on-disk backends.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Objects written through a registered [`MemOdb`] are visible only in memory until flushed;
    /// after a flush a fresh repository (no backend) must find them on disk.
    #[test]
    fn flush_writes_objects_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        store.register(&repo);

        let ids: Vec<Oid> = (0..4)
            .map(|i| repo.blob(format!("in-memory blob {i}").as_bytes()).unwrap())
            .collect();

        // Readable in-process before the flush, but only from memory.
        for id in &ids {
            assert!(repo.odb().unwrap().exists(*id));
        }

        store.flush().unwrap();

        // A fresh repo with no backend can only see objects that made it to disk.
        let on_disk = git2::Repository::open(dir.path()).unwrap();
        for (i, id) in ids.iter().enumerate() {
            let blob = on_disk.find_blob(*id).unwrap();
            assert_eq!(blob.content(), format!("in-memory blob {i}").as_bytes());
        }

        // The store is drained: a second flush is a no-op.
        store.flush().unwrap();
    }

    /// A linked worktree's gitdir has no `objects/` of its own, so [`crate::pack::objects_dir`]
    /// must resolve the common object directory rather than `repo.path()/objects` (which does not
    /// exist and previously failed with "No such file or directory").
    #[test]
    fn flush_writes_to_common_dir_for_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let main_path = tmp.path().join("main");
        let repo = git2::Repository::init(&main_path).unwrap();

        // A worktree can only be added once HEAD points at a commit.
        let sig = git2::Signature::now("t", "t@t").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        drop(tree);

        let wt_path = tmp.path().join("wt");
        repo.worktree("wt", &wt_path, None).unwrap();
        let wt_repo = git2::Repository::open(&wt_path).unwrap();

        // The worktree's gitdir differs from its common dir (the main gitdir).
        assert_ne!(wt_repo.path(), wt_repo.commondir());

        let store = MemOdb::new(None, crate::pack::objects_dir(&wt_repo));
        store.register(&wt_repo);
        let id = wt_repo.blob(b"worktree blob").unwrap();

        // Must not fail on the (nonexistent) per-worktree objects/pack directory.
        store.flush().unwrap();

        // The pack landed in the common object dir, so the main repo can read the blob from disk.
        let main = git2::Repository::open(&main_path).unwrap();
        assert_eq!(main.find_blob(id).unwrap().content(), b"worktree blob");
    }

    /// When `limit` is set, writing enough data to exceed it enqueues a background pack from inside
    /// the write path, which lands the objects on disk asynchronously (leaving the store to drain
    /// the rest at the transaction boundary). Each further overflow enqueues another pack.
    #[test]
    fn flushes_on_overflow_during_writes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // A 16-byte limit: each 100-byte blob overflows it, so every write enqueues a pack.
        let store = MemOdb::new(Some(16), crate::pack::objects_dir(&repo));
        store.register(&repo);

        // The write overflowed and enqueued a background pack; it lands on the flusher thread, so
        // poll a fresh on-disk view until the object appears.
        let id1 = repo.blob(&b"x".repeat(100)).unwrap();
        assert!(
            wait_on_disk(dir.path(), id1),
            "overflow pack never reached disk"
        );

        // A second object overflows again, enqueueing another pack.
        let id2 = repo.blob(&b"y".repeat(100)).unwrap();
        assert!(
            wait_on_disk(dir.path(), id2),
            "second overflow pack never reached disk"
        );
    }

    /// A flush leaves exactly a `pack-<hash>.pack`/`.idx` pair behind — in particular no `.keep`
    /// file, which gix-pack creates for its caller to remove and which would otherwise exempt the
    /// pack from `git repack -d` (and churn test dir listings).
    #[test]
    fn flush_leaves_only_pack_and_idx() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));
        store.register(&repo);

        repo.blob(b"some blob").unwrap();
        store.flush().unwrap();

        let mut exts: Vec<String> = std::fs::read_dir(crate::pack::objects_dir(&repo).join("pack"))
            .unwrap()
            .map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                assert!(name.starts_with("pack-"), "unexpected file {name}");
                name.rsplit('.').next().unwrap().to_string()
            })
            .collect();
        exts.sort();
        assert_eq!(exts, ["idx", "pack"]);
    }

    /// Poll a freshly-opened (backend-less) view of the repository until `id` is readable from disk,
    /// up to ~2s. Used to observe asynchronous background packs without racing the flusher thread.
    fn wait_on_disk(repo_path: &std::path::Path, id: Oid) -> bool {
        for _ in 0..200 {
            if let Ok(repo) = git2::Repository::open(repo_path) {
                if repo.find_blob(id).is_ok() {
                    return true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }
}
