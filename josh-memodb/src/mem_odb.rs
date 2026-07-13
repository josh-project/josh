//! A per-operation in-memory ODB store buffering the objects josh produces while filtering, instead
//! of writing each as a loose object. Packed to a packfile at transaction and external-git
//! boundaries, and additionally packed mid-transaction once the buffered object data exceeds the
//! configured size limit (so a single transaction may write several packfiles). One [`MemOdb`] per
//! operation (not process-global), so flushes are isolated and deterministic.
//!
//! Packing itself runs on a background thread (see [`crate::flusher`]): the write path enqueues the
//! work and keeps filtering, and a boundary flush blocks only until the pack is durable. The store's
//! `Mutex` therefore guards concurrent access from the filter thread (writes/reads through the
//! backend) and the flusher thread (which reads objects to pack them, then evicts them); it is
//! `Send + Sync` so its `Arc` can cross to the flusher.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use git2::{ErrorClass, ErrorCode, ObjectType, Oid};
use libgit2_sys as raw;

use crate::odb_backend::{self, OdbBackend};

type ObjectMap = BTreeMap<Oid, (raw::git_object_t, Box<[u8]>)>;

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
    /// Repository the store is registered on, re-opened by the background flusher to run the
    /// packbuilder (a `git2::Repository` is not `Send`).
    repo_path: PathBuf,
    /// Set while an overflow chunk is queued on or running in the background flusher, so the write
    /// path does not pile up redundant chunk requests every time the store crosses its limit.
    /// Cleared by the flusher once the chunk has packed and evicted.
    chunk_in_flight: AtomicBool,
}

impl MemOdb {
    /// Create an empty store. Returned as an [`Arc`] because the store is shared between the owning
    /// transaction and the libgit2 backend registered on its repository. `limit` bounds the total
    /// buffered object data: once exceeded the store flushes itself to a packfile (`None` = unbounded).
    pub fn new(limit: Option<usize>, repo_path: PathBuf) -> Arc<MemOdb> {
        Arc::new(MemOdb {
            inner: Mutex::new(Inner {
                map: Default::default(),
                size: 0,
            }),
            limit,
            repo_path,
            chunk_in_flight: AtomicBool::new(false),
        })
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

    /// Buffer `oid` (a no-op for a content-addressed duplicate) and return whether the store has now
    /// exceeded its size limit, so the caller can flush.
    fn insert(&self, oid: Oid, kind: raw::git_object_t, data: Box<[u8]>) -> bool {
        let len = data.len();
        let mut inner = self.inner.lock().unwrap();
        if inner.map.insert(oid, (kind, data)).is_none() {
            inner.size += len;
        }
        self.limit.is_some_and(|limit| inner.size > limit)
    }

    /// Drain every in-memory object into a packfile on disk and evict it from the store, blocking
    /// until done. Called when the owning transaction completes (or at an explicit external-git
    /// boundary) so the on-disk repository is left whole for any subsequent process that expects the
    /// objects on disk. Packing runs on the background flusher, behind any overflow chunks already
    /// queued for this store, so it returns only once every buffered object is durable.
    pub fn flush(self: &Arc<Self>) -> Result<(), git2::Error> {
        crate::flusher::drain(self.clone())
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

    /// Pack this store's currently-buffered objects into a packfile and evict them. Runs only on the
    /// background flusher, which owns no repository handle (`git2::Repository` is not `Send`), so it
    /// re-opens the repository and registers a backend sharing this store — the packbuilder reads
    /// each object back through that backend.
    pub(crate) fn pack_to_disk(self: &Arc<Self>) -> Result<(), git2::Error> {
        // Snapshot the oids under the lock, then release it. The objects must stay in the store
        // across the packbuilder below: `pb.write` reads each one back through the odb -> this
        // backend -> `self.inner.lock()`, so we can neither hold the lock here (self-deadlock) nor
        // drain the map before the pack is on disk. A BTreeMap iterates in sorted oid order, so the
        // packbuilder produces a deterministic packfile (hence a deterministic pack name).
        let oids: Vec<Oid> = {
            let inner = self.inner.lock().unwrap();
            if inner.map.is_empty() {
                return Ok(());
            }
            inner.map.keys().copied().collect()
        };

        let repo = git2::Repository::open(&self.repo_path)?;
        self.register(&repo);

        // The memory-only freshen (see `odb_backend`) no longer deduplicates writes against disk,
        // so objects already present on disk may have been re-buffered into the store. Pack only the
        // genuinely-new ones — `filter_absent_on_disk` preserves the sorted oid order, so the
        // packfile (and its name) stays deterministic.
        let to_pack = odb_backend::filter_absent_on_disk(&repo, &oids);
        if !to_pack.is_empty() {
            let mut pb = repo.packbuilder()?;
            for oid in &to_pack {
                pb.insert_object(*oid, None)?;
            }
            // `packfile_path` resolves the common object directory, so this is correct for linked
            // worktrees (whose gitdir has no `objects/` of its own) as well as normal repos.
            pb.write(&crate::pack::packfile_path(&repo), 0)?;
        }

        // Evict exactly the snapshotted oids (now durable: packed just above, or already on disk).
        // Writes that landed after the snapshot stay buffered for the next chunk or the drain, so a
        // background chunk running concurrently with the write path never drops a live object.
        let mut inner = self.inner.lock().unwrap();
        for oid in &oids {
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
            .inner
            .lock()
            .unwrap()
            .map
            .get(&oid)
            .map(|(kind, data)| (data.len(), raw_to_kind(*kind)))
            .ok_or_else(not_found)
    }

    fn read(&self, oid: Oid) -> Result<(Vec<u8>, ObjectType), git2::Error> {
        self.store
            .inner
            .lock()
            .unwrap()
            .map
            .get(&oid)
            .map(|(kind, data)| (data.to_vec(), raw_to_kind(*kind)))
            .ok_or_else(not_found)
    }

    fn write(&mut self, oid: Oid, data: Vec<u8>, kind: ObjectType) -> Result<(), git2::Error> {
        let overflow = self.store.insert(oid, kind.raw(), data.into_boxed_slice());
        if overflow {
            self.store.enqueue_chunk();
        }
        Ok(())
    }

    fn exists(&self, oid: Oid) -> bool {
        self.store.inner.lock().unwrap().map.contains_key(&oid)
    }

    fn exists_prefix(&self, _oid: Oid, _oid_len: usize) -> Option<Oid> {
        // Abbreviated-OID lookup against the in-memory store is not implemented; full-OID lookups
        // (the hot path) are unaffected, and prefix lookups fall through to the on-disk backends.
        None
    }
}

fn raw_to_kind(kind: raw::git_object_t) -> ObjectType {
    ObjectType::from_raw(kind).expect("stored objects always have a valid git type")
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

        let store = MemOdb::new(None, repo.path().to_owned());
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

    /// A linked worktree's gitdir has no `objects/` of its own, so the flush must write into the
    /// common object directory rather than `repo.path()/objects/pack` (which does not exist and
    /// previously failed with "No such file or directory").
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

        let store = MemOdb::new(None, wt_repo.path().to_owned());
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
        let store = MemOdb::new(Some(16), repo.path().to_owned());
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
