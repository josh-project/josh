//! An in-memory ODB store buffering the objects josh produces while filtering, instead of writing
//! each as a loose object. There is one store per objects directory (see [`crate::registry`]), so a
//! store outlives the transactions that write to it and the next transaction on the same repository
//! reads their objects from memory rather than from a pack.
//!
//! Packing is therefore not tied to a transaction. A store packs when its buffered data exceeds the
//! configured size limit, when a transaction that published a ref ends, at an explicit boundary
//! ([`MemOdb::flush`]), and at process exit ([`crate::FlushGuard`]).
//!
//! Packing itself runs on a background thread (see [`crate::flusher`]): the write path enqueues the
//! work and keeps filtering, and a boundary flush blocks only until the pack is durable. The store's
//! locks therefore guard concurrent access from every thread holding a transaction on the repository
//! and from the flusher thread (which snapshots the buffered objects to pack them, then evicts
//! them); the store is `Send + Sync` so its `Arc` can cross to the flusher.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::{Mutex, RwLock};

use gix_hash::ObjectId;
use gix_object::Kind;

/// Object bytes are held behind an `Arc` so a flush can snapshot them without copying: the store
/// must keep serving reads while the flusher packs (eviction only happens once the pack is
/// durable), so the snapshot shares the buffers instead of draining or cloning them.
type ObjectMap = BTreeMap<ObjectId, (Kind, Arc<[u8]>)>;

/// A `Send` snapshot of buffered objects in sorted-oid order (the `ObjectMap` iteration order),
/// which keeps the resulting packfile — and hence its checksum-derived name — deterministic.
pub(crate) type Snapshot = Vec<(ObjectId, Kind, Arc<[u8]>)>;

/// `limit` sentinel for an unbounded store, so the limit can be tightened atomically.
const UNBOUNDED: usize = usize::MAX;

/// The buffered objects plus a running total of their data size, guarded by one lock so that an
/// insert and the overflow check following it are observed atomically.
struct Inner {
    map: ObjectMap,
    size: usize,
}

/// An in-memory object store. Obtain the store of a repository with
/// [`crate::registry::shared`], or an unregistered one with [`MemOdb::new`] /
/// [`MemOdb::chained`]; drain it to disk with [`MemOdb::flush`]. When a limit is set, the store
/// also enqueues a background pack from the write path as soon as the buffered data exceeds it
/// (see [`MemOdb::enqueue_chunk`]).
pub struct MemOdb {
    inner: RwLock<Inner>,
    /// The tightest buffered-data bound any attacher asked for, [`UNBOUNDED`] when none did. A
    /// shared store serves transactions opened with different limits, so the bound is a budget
    /// for the objects directory and only ever tightens (see [`MemOdb::tighten_limit`]).
    limit: AtomicUsize,
    /// The store's resolved object directory (`<commondir>/objects`, see
    /// [`crate::pack::objects_dir`]), captured while the caller holds a repository handle; the
    /// background flusher packs straight into it without opening one.
    objects_dir: PathBuf,
    /// Set while an overflow chunk is queued on or running in the background flusher, so the write
    /// path does not pile up redundant chunk requests every time the store crosses its limit.
    /// Cleared by the flusher once the chunk has packed and evicted.
    chunk_in_flight: AtomicBool,
    /// Held for the duration of a pack, so two packers never snapshot and evict the same store
    /// concurrently. The flusher thread serialises its own jobs, but [`crate::FlushGuard`] packs
    /// inline at process exit and would otherwise race it.
    pack_lock: Mutex<()>,
    /// Consulted on a read miss, for stores that must not contribute their own objects to the
    /// repository's registered store but must still see what it holds (see [`MemOdb::chained`]).
    read_through: Option<Arc<MemOdb>>,
}

impl MemOdb {
    /// Create an empty, unregistered store: its objects are visible only through this handle.
    /// Prefer [`crate::registry::shared`] for a repository josh filters into; this is for stores
    /// with a lifetime of their own (the distributed cache backend's repository, tests). `limit`
    /// bounds the total buffered object data, above which the store packs itself in the
    /// background (`None` = unbounded); `objects_dir` is where flushes land.
    pub fn new(limit: Option<usize>, objects_dir: PathBuf) -> Arc<MemOdb> {
        Self::build(limit, objects_dir, None)
    }

    /// [`MemOdb::new`] plus a store to read through on a miss. Used for ephemeral transactions:
    /// their objects are discarded, so they must not land in the repository's registered store,
    /// but they must still see the objects it buffers.
    pub fn chained(
        limit: Option<usize>,
        objects_dir: PathBuf,
        read_through: Arc<MemOdb>,
    ) -> Arc<MemOdb> {
        Self::build(limit, objects_dir, Some(read_through))
    }

    pub(crate) fn build(
        limit: Option<usize>,
        objects_dir: PathBuf,
        read_through: Option<Arc<MemOdb>>,
    ) -> Arc<MemOdb> {
        Arc::new(MemOdb {
            inner: RwLock::new(Inner {
                map: Default::default(),
                size: 0,
            }),
            limit: AtomicUsize::new(limit.unwrap_or(UNBOUNDED)),
            objects_dir,
            chunk_in_flight: AtomicBool::new(false),
            pack_lock: Mutex::new(()),
            read_through,
        })
    }

    /// Lower the bound to `limit` if that is tighter. Called on every attach: honouring the
    /// tightest request is the safe direction, and the bound never loosens.
    pub(crate) fn tighten_limit(&self, limit: Option<usize>) {
        self.limit
            .fetch_min(limit.unwrap_or(UNBOUNDED), Ordering::AcqRel);
    }

    /// Buffer `oid` and return whether the store has now exceeded its size limit, so the caller
    /// can flush. A content-addressed duplicate is a no-op and never reports overflow: it adds
    /// no bytes, so it must not trigger a pack.
    fn insert(&self, oid: ObjectId, kind: Kind, data: Arc<[u8]>) -> bool {
        let len = data.len();
        let mut inner = self.inner.write();
        if inner.map.insert(oid, (kind, data)).is_some() {
            return false;
        }
        inner.size += len;
        inner.size > self.limit.load(Ordering::Acquire)
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
        if let Some(hit) = self
            .inner
            .read()
            .map
            .get(id)
            .map(|(kind, data)| (*kind, data.clone()))
        {
            return Some(hit);
        }
        self.read_through.as_ref()?.get(id)
    }

    /// Kind and size of the buffered object `id`, without touching its bytes.
    pub fn header(&self, id: &gix_hash::oid) -> Option<(Kind, u64)> {
        if let Some(hit) = self
            .inner
            .read()
            .map
            .get(id)
            .map(|(kind, data)| (*kind, data.len() as u64))
        {
            return Some(hit);
        }
        self.read_through.as_ref()?.header(id)
    }

    pub fn contains(&self, id: &gix_hash::oid) -> bool {
        self.inner.read().map.contains_key(id)
            || self.read_through.as_ref().is_some_and(|s| s.contains(id))
    }

    /// Whether this store holds nothing of its own to pack.
    fn is_empty(&self) -> bool {
        self.inner.read().map.is_empty()
    }

    /// Drain every in-memory object this store can see into a packfile on disk and evict it,
    /// blocking until done: at an external-git boundary, and when a transaction that published a
    /// ref ends. Everything *readable* through the store has to become durable, so a chained
    /// store drains what it reads through too. Runs on the background flusher behind any queued
    /// overflow chunks; a store holding nothing skips the round trip.
    pub fn flush(self: &Arc<Self>) -> anyhow::Result<()> {
        if let Some(behind) = &self.read_through {
            behind.flush()?;
        }
        if self.is_empty() {
            return Ok(());
        }
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
    /// that follow into a single queued chunk.
    fn enqueue_chunk(self: &Arc<Self>) {
        if self.chunk_in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        crate::flusher::enqueue_chunk(self.clone());
    }

    /// Release the in-flight guard after a background chunk. A write can overflow after the worker
    /// snapshots the store but before it releases the guard; check the remaining size after the
    /// release so that write still schedules the next chunk. Failed packs wait for a later write or
    /// explicit drain instead of retrying in a hot loop.
    pub(crate) fn finish_chunk(self: &Arc<Self>, packed: bool) {
        self.chunk_in_flight.store(false, Ordering::Release);
        if packed && self.inner.read().size > self.limit.load(Ordering::Acquire) {
            self.enqueue_chunk();
        }
    }

    /// Pack this store's currently-buffered objects into a packfile and evict them. Runs on the
    /// background flusher, or inline at process exit, packing a snapshot of the buffers straight
    /// into the object directory captured at construction (see [`crate::pack::write_snapshot`]) —
    /// no repository handle involved. The snapshot shares the object buffers (`Arc`), and the
    /// objects stay in the map while the pack is written, so concurrent reads keep resolving until
    /// eviction — which happens only once the pack is on disk.
    pub(crate) fn pack_to_disk(self: &Arc<Self>) -> anyhow::Result<()> {
        // One packer per store at a time: the flusher's own jobs are already serialised, but the
        // process-exit flush packs inline and would otherwise snapshot and evict concurrently.
        let _packing = self.pack_lock.lock();

        // Snapshot under the lock, then release it: `write_snapshot` filters against the on-disk
        // store and compresses every object, which must not stall the filter threads' reads and
        // writes. Objects already on disk may have been re-buffered (the write gate does not
        // deduplicate against the repository's own disk, see `crate::odb::Odb::write`);
        // `write_snapshot` packs only the genuinely-new ones, in the map's sorted oid order,
        // keeping the packfile — and its checksum-derived name — deterministic.
        let snapshot: Snapshot = {
            let inner = self.inner.read();
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
        let mut inner = self.inner.write();
        for (oid, _, _) in &snapshot {
            if let Some((_, data)) = inner.map.remove(oid) {
                inner.size = inner.size.saturating_sub(data.len());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Objects written to a [`MemOdb`] stay in memory until flushed; after a flush a fresh
    /// repository must find them on disk.
    #[test]
    fn flush_writes_objects_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();

        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));

        let ids: Vec<ObjectId> = (0..4)
            .map(|i| store.write(Kind::Blob, format!("in-memory blob {i}").as_bytes()))
            .collect();

        for id in &ids {
            assert!(store.contains(id));
            assert!(
                disk_blob(dir.path(), *id).is_none(),
                "buffered object must not be on disk before the flush"
            );
        }

        store.flush().unwrap();

        // A fresh repo can only see objects that made it to disk.
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(
                disk_blob(dir.path(), *id).unwrap(),
                format!("in-memory blob {i}").as_bytes()
            );
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
        let repo = gix::init(&main_path).unwrap();

        // Set up the on-disk linked-worktree metadata. Gitoxide reads the same `gitdir` and
        // `commondir` files that `git worktree add` creates; no checkout is needed for this test.
        let wt_path = tmp.path().join("wt");
        let wt_admin = repo.git_dir().join("worktrees").join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        std::fs::create_dir_all(&wt_admin).unwrap();
        std::fs::write(
            wt_path.join(".git"),
            format!("gitdir: {}\n", wt_admin.display()),
        )
        .unwrap();
        std::fs::write(wt_admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            wt_admin.join("gitdir"),
            format!("{}\n", wt_path.join(".git").display()),
        )
        .unwrap();
        std::fs::write(wt_admin.join("HEAD"), "ref: refs/heads/master\n").unwrap();

        let wt_repo = gix::open(&wt_path).unwrap();

        // The worktree's gitdir differs from its common dir (the main gitdir).
        assert_ne!(wt_repo.git_dir(), wt_repo.common_dir());

        let store = MemOdb::new(None, crate::pack::objects_dir(&wt_repo));
        let id = store.write(Kind::Blob, b"worktree blob");

        // Must not fail on the (nonexistent) per-worktree objects/pack directory.
        store.flush().unwrap();

        // The pack landed in the common object dir, so the main repo can read the blob from disk.
        assert_eq!(disk_blob(&main_path, id).unwrap(), b"worktree blob");
    }

    /// When `limit` is set, writing enough data to exceed it enqueues a background pack from inside
    /// the write path, which lands the objects on disk asynchronously (leaving the store to drain
    /// the rest at the next barrier). Each further overflow enqueues another pack.
    #[test]
    fn flushes_on_overflow_during_writes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();

        // A 16-byte limit: each 100-byte blob overflows it, so every write enqueues a pack.
        let store = MemOdb::new(Some(16), crate::pack::objects_dir(&repo));

        // The write overflowed and enqueued a background pack; it lands on the flusher thread, so
        // poll a fresh on-disk view until the object appears.
        let id1 = store.write(Kind::Blob, &b"x".repeat(100));
        assert!(
            wait_on_disk(dir.path(), id1),
            "overflow pack never reached disk"
        );

        // A second object overflows again, enqueueing another pack.
        let id2 = store.write(Kind::Blob, &b"y".repeat(100));
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
        let repo = gix::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));

        store.write(Kind::Blob, b"some blob");
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

    /// A chained store reads what the store behind it holds and never adopts its objects, so
    /// packing it leaves that store untouched -- the contract ephemeral transactions rely on.
    /// An explicit flush is a durability barrier for everything readable, so it drains the chain.
    #[test]
    fn chained_store_reads_through_and_packs_separately() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();

        let shared = MemOdb::new(None, crate::pack::objects_dir(&repo));
        let behind = shared.write(Kind::Blob, b"shared blob");

        let private = MemOdb::chained(None, crate::pack::objects_dir(&repo), shared.clone());
        let own = private.write(Kind::Blob, b"private blob");

        assert!(private.contains(&behind));
        assert_eq!(&*private.get(&behind).unwrap().1, b"shared blob");
        assert_eq!(private.header(&behind).unwrap().0, Kind::Blob);
        assert!(!shared.contains(&own));

        // Packing the private store writes only its own objects.
        private.pack_to_disk().unwrap();
        assert!(disk_blob(dir.path(), own).is_some());
        assert!(disk_blob(dir.path(), behind).is_none());
        assert!(shared.contains(&behind));

        // A flush is a barrier for everything the store can see, chain included.
        private.flush().unwrap();
        assert!(!shared.contains(&behind));
        assert!(disk_blob(dir.path(), behind).is_some());
    }

    /// The limit only ever tightens, whatever order attachers ask in, so a shared store honours
    /// the smallest budget requested for its objects directory.
    #[test]
    fn limit_only_tightens() {
        let dir = tempfile::tempdir().unwrap();
        let repo = gix::init(dir.path()).unwrap();
        let store = MemOdb::new(None, crate::pack::objects_dir(&repo));

        assert_eq!(store.limit.load(Ordering::Acquire), UNBOUNDED);
        store.tighten_limit(Some(64));
        assert_eq!(store.limit.load(Ordering::Acquire), 64);
        store.tighten_limit(Some(128));
        assert_eq!(store.limit.load(Ordering::Acquire), 64);
        store.tighten_limit(None);
        assert_eq!(store.limit.load(Ordering::Acquire), 64);
        store.tighten_limit(Some(16));
        assert_eq!(store.limit.load(Ordering::Acquire), 16);
    }

    /// Poll a freshly-opened view of the repository until `id` is readable from disk,
    /// up to ~2s. Used to observe asynchronous background packs without racing the flusher thread.
    fn wait_on_disk(repo_path: &std::path::Path, id: ObjectId) -> bool {
        for _ in 0..200 {
            if disk_blob(repo_path, id).is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
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
