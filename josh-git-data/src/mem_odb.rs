//! A per-operation in-memory ODB store buffering the objects josh produces while filtering, instead
//! of writing each as a loose object. Flushed to a packfile at transaction and external-git
//! boundaries, and additionally flushed mid-transaction once the buffered object data exceeds the
//! configured size limit (so a single transaction may write several packfiles). One [`MemOdb`] per
//! operation (not process-global), so flushes are isolated and deterministic. Access to a store is
//! single-threaded, but it is `Send + Sync` to cross async boundaries.

use std::collections::BTreeMap;
use std::path::PathBuf;
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
/// [`MemOdb::flush`]. When `limit` is set, the store also drains itself from the write path as soon
/// as the buffered data exceeds it (see [`MemOdb::flush_on_overflow`]).
pub struct MemOdb {
    inner: Mutex<Inner>,
    limit: Option<usize>,
    /// Repository the store is registered on, re-opened for overflow flushes
    /// (see [`MemOdb::flush_on_overflow`]).
    repo_path: PathBuf,
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
        })
    }

    /// Register a backend on `repo`'s object database that reads from / writes to this store, at a
    /// priority above the on-disk loose (2) and pack (1) backends, so writes land in memory and reads
    /// check memory first.
    pub fn register(self: &Arc<Self>, repo: &git2::Repository) {
        let _ = odb_backend::register(
            repo,
            1000,
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

    /// Drain every in-memory object into a packfile on disk and evict it from the store. Called when
    /// the owning transaction completes (or at an explicit external-git boundary) so the on-disk
    /// repository is left whole for any subsequent process that expects the objects on disk.
    pub fn flush(&self, repo: &git2::Repository) -> Result<(), git2::Error> {
        self.flush_into(repo)
    }

    /// Flush triggered from the ODB `write` path when the store overflows its limit. The write
    /// callback has no repository handle, and the packbuilder reads each object back through a
    /// registered backend, so re-open the repository, register a backend sharing this store on it,
    /// and flush through that.
    fn flush_on_overflow(self: &Arc<Self>) -> Result<(), git2::Error> {
        let repo = git2::Repository::open(&self.repo_path)?;
        odb_backend::register(
            &repo,
            1000,
            MemBackend {
                store: self.clone(),
            },
        )?;
        self.flush_into(&repo)
    }

    fn flush_into(&self, repo: &git2::Repository) -> Result<(), git2::Error> {
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

        let mut pb = repo.packbuilder()?;
        for oid in &oids {
            pb.insert_object(*oid, None)?;
        }
        // `packfile_path` resolves the common object directory, so this is correct for linked
        // worktrees (whose gitdir has no `objects/` of its own) as well as normal repos.
        pb.write(&crate::pack::packfile_path(repo), 0)?;

        // Dropping the whole map (rather than only the flushed oids) is safe because a store is
        // never flushed with writes in flight.
        let mut inner = self.inner.lock().unwrap();
        inner.map.clear();
        inner.size = 0;
        Ok(())
    }
}

/// The libgit2 backend handle for a [`MemOdb`]: a cheap [`Arc`] onto the store, registered on one
/// repository by [`MemOdb::register`].
struct MemBackend {
    store: Arc<MemOdb>,
}

fn not_found() -> git2::Error {
    // ErrorCode::NotFound maps to GIT_ENOTFOUND, so a miss makes libgit2 fall through to the next
    // backend (the on-disk pack/loose backends) instead of treating it as a hard failure.
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
            self.store.flush_on_overflow()?;
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

        store.flush(&repo).unwrap();

        // A fresh repo with no backend can only see objects that made it to disk.
        let on_disk = git2::Repository::open(dir.path()).unwrap();
        for (i, id) in ids.iter().enumerate() {
            let blob = on_disk.find_blob(*id).unwrap();
            assert_eq!(blob.content(), format!("in-memory blob {i}").as_bytes());
        }

        // The store is drained: a second flush is a no-op.
        store.flush(&repo).unwrap();
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
        store.flush(&wt_repo).unwrap();

        // The pack landed in the common object dir, so the main repo can read the blob from disk.
        let main = git2::Repository::open(&main_path).unwrap();
        assert_eq!(main.find_blob(id).unwrap().content(), b"worktree blob");
    }

    /// When `limit` is set, writing enough data to exceed it flushes the store to a packfile
    /// mid-transaction (from inside the write path), leaving the objects on disk and the store
    /// empty. Each further overflow writes another packfile.
    #[test]
    fn flushes_on_overflow_during_writes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // A 16-byte limit: each 100-byte blob overflows it, so every write flushes.
        let store = MemOdb::new(Some(16), repo.path().to_owned());
        store.register(&repo);

        let id1 = repo.blob(&b"x".repeat(100)).unwrap();
        // The write overflowed and flushed: id1 is already on disk, and a fresh repo can read it.
        let on_disk = git2::Repository::open(dir.path()).unwrap();
        assert_eq!(on_disk.find_blob(id1).unwrap().content(), &b"x".repeat(100));

        // A second object overflows again, producing a second packfile.
        let id2 = repo.blob(&b"y".repeat(100)).unwrap();
        assert_eq!(on_disk.find_blob(id2).unwrap().content(), &b"y".repeat(100));
    }
}
