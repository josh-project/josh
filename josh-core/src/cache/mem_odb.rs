//! In-memory git object database backend.
//!
//! josh produces enormous numbers of git objects while filtering, and writing each as a loose
//! object (a separate file + zlib stream + directory insert) dominates the cost of large rewrites.
//! This module installs a custom libgit2 ODB backend, registered at high priority on every
//! repository josh opens, that keeps written objects in a process-global concurrent map and serves
//! read-back from memory. Objects are flushed to packfiles separately; reads that miss the map
//! fall through to the on-disk pack/loose backends automatically.

use std::ffi::{c_int, c_void};
use std::path::Path;
use std::sync::Arc;

use libgit2_sys as raw;

/// Process-global in-memory object store shared by every backend instance. Every per-thread
/// repository josh opens registers its own backend pointing at this one store, mirroring how the
/// threads previously shared state through the filesystem.
pub(crate) static STORE: std::sync::LazyLock<Arc<MemOdb>> =
    std::sync::LazyLock::new(|| Arc::new(MemOdb::new()));

type Map = scc::HashMap<Key, (raw::git_object_t, Arc<[u8]>), OidHasherBuilder>;

pub(crate) struct MemOdb {
    map: Map,
}

impl MemOdb {
    fn new() -> Self {
        MemOdb {
            map: Map::with_hasher(OidHasherBuilder),
        }
    }

    fn insert(&self, oid: git2::Oid, kind: raw::git_object_t, data: Arc<[u8]>) {
        // A duplicate key means the identical (content-addressed) object is already present.
        let _ = self.map.insert_sync(Key(oid), (kind, data));
    }
}

/// Drain every in-memory object into a packfile on disk and evict it from the store. Called when
/// the outermost transaction completes so the on-disk repository is left whole for any subsequent
/// process (e.g. a `git` subprocess) that expects the objects on disk. The packbuilder reads object
/// contents back through `repo`'s odb, which must have this backend registered.
pub(crate) fn flush_all(repo: &git2::Repository) -> Result<(), git2::Error> {
    let mut oids = Vec::new();
    STORE.map.iter_sync(|k, _| {
        oids.push(k.0);
        true
    });
    if oids.is_empty() {
        return Ok(());
    }
    // scc iteration order is unspecified; sort so the packbuilder receives objects in a stable
    // order and produces a deterministic packfile (hence a deterministic pack name).
    oids.sort();

    let mut pb = repo.packbuilder()?;
    for oid in &oids {
        pb.insert_object(*oid, None)?;
    }
    pb.write(&repo.path().join("objects").join("pack"), 0)?;

    for oid in &oids {
        STORE.map.remove_sync(&Key(*oid));
    }
    Ok(())
}

/// Like [`flush_all`] but for callers that only have a repository path: opens the repo, attaches
/// the backend so the packbuilder can read in-memory objects, and flushes. Used at external-git
/// boundaries (e.g. before spawning a `git` subprocess) where no live transaction is in hand.
pub(crate) fn flush_all_at(repo_path: &Path) -> anyhow::Result<()> {
    if STORE.map.is_empty() {
        return Ok(());
    }
    let repo = git2::Repository::open(repo_path)?;
    register(&repo);
    flush_all(&repo)?;
    Ok(())
}

/// Map key wrapping a git `Oid` with a single-`write` `Hash` impl, so [`OidHasher`] sees the raw
/// digest bytes in one call rather than byte-by-byte.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key(git2::Oid);

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write(self.0.as_bytes());
    }
}

/// Passthrough hasher: an OID is already a cryptographic digest, so its first 8 bytes are a perfect
/// hash. Running SipHash over them would be wasted work on the hot lookup path.
#[derive(Clone, Default)]
struct OidHasherBuilder;

impl std::hash::BuildHasher for OidHasherBuilder {
    type Hasher = OidHasher;
    fn build_hasher(&self) -> OidHasher {
        OidHasher(0)
    }
}

struct OidHasher(u64);

impl std::hash::Hasher for OidHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(8);
        let mut b = [0u8; 8];
        b[..n].copy_from_slice(&bytes[..n]);
        self.0 = u64::from_le_bytes(b);
    }
}

/// libgit2 backend object. `base` must be first so that a `*mut git_odb_backend` handed to a
/// callback can be cast back to `*mut JoshBackend`. libgit2 owns the allocation and releases it
/// through the `free` callback.
#[repr(C)]
struct JoshBackend {
    base: raw::git_odb_backend,
    store: Arc<MemOdb>,
}

/// Register the in-memory backend on `repo`'s object database at a priority above the on-disk
/// loose (2) and pack (1) backends, so writes land in memory and reads check memory first.
///
/// git2 does not expose the raw `git_odb`/`git_repository` pointers (its `Binding` trait lives in a
/// private module), so we read the pointer out of the single-field `Repository` newtype. This is
/// sound only because josh pins `git2` exactly; a version bump must re-verify the layout.
pub(crate) fn register(repo: &git2::Repository) {
    unsafe {
        let repo_raw = *(repo as *const git2::Repository as *const *mut raw::git_repository);
        let mut odb: *mut raw::git_odb = std::ptr::null_mut();
        if raw::git_repository_odb(&mut odb, repo_raw) != 0 {
            return;
        }
        let backend = new_backend();
        if raw::git_odb_add_backend(odb, backend, 1000) != 0 {
            drop(Box::from_raw(backend as *mut JoshBackend));
        }
        // git_repository_odb handed us an owned reference; the odb itself lives on inside the repo.
        raw::git_odb_free(odb);
    }
}

fn new_backend() -> *mut raw::git_odb_backend {
    let mut be = Box::new(JoshBackend {
        // All-zero is a valid git_odb_backend: null odb pointer and `None` for every callback.
        base: unsafe { std::mem::zeroed() },
        store: STORE.clone(),
    });
    unsafe {
        raw::git_odb_init_backend(&mut be.base, raw::GIT_ODB_BACKEND_VERSION);
    }
    be.base.read = Some(odb_read);
    be.base.read_header = Some(odb_read_header);
    be.base.write = Some(odb_write);
    be.base.exists = Some(odb_exists);
    be.base.free = Some(odb_free);
    Box::into_raw(be) as *mut raw::git_odb_backend
}

unsafe fn backend_store<'a>(backend: *mut raw::git_odb_backend) -> &'a MemOdb {
    unsafe { &(*(backend as *const JoshBackend)).store }
}

unsafe fn oid_to_key(oid: *const raw::git_oid) -> Key {
    Key(git2::Oid::from_bytes(unsafe { &(*oid).id }).expect("git_oid is 20 raw bytes"))
}

extern "C" fn odb_read(
    buffer_p: *mut *mut c_void,
    len_p: *mut usize,
    type_p: *mut raw::git_object_t,
    backend: *mut raw::git_odb_backend,
    oid: *const raw::git_oid,
) -> c_int {
    unsafe {
        let store = backend_store(backend);
        let key = oid_to_key(oid);
        match store
            .map
            .read_sync(&key, |_, (kind, data)| (*kind, data.clone()))
        {
            Some((kind, data)) => {
                let buf = raw::git_odb_backend_data_alloc(backend, data.len());
                if buf.is_null() {
                    return raw::GIT_ERROR;
                }
                std::ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, data.len());
                *buffer_p = buf;
                *len_p = data.len();
                *type_p = kind;
                raw::GIT_OK
            }
            None => raw::GIT_ENOTFOUND,
        }
    }
}

extern "C" fn odb_read_header(
    len_p: *mut usize,
    type_p: *mut raw::git_object_t,
    backend: *mut raw::git_odb_backend,
    oid: *const raw::git_oid,
) -> c_int {
    unsafe {
        let store = backend_store(backend);
        let key = oid_to_key(oid);
        match store
            .map
            .read_sync(&key, |_, (kind, data)| (*kind, data.len()))
        {
            Some((kind, len)) => {
                *len_p = len;
                *type_p = kind;
                raw::GIT_OK
            }
            None => raw::GIT_ENOTFOUND,
        }
    }
}

extern "C" fn odb_write(
    backend: *mut raw::git_odb_backend,
    oid: *const raw::git_oid,
    data: *const c_void,
    len: usize,
    kind: raw::git_object_t,
) -> c_int {
    unsafe {
        let store = backend_store(backend);
        let key = oid_to_key(oid);
        let bytes: Arc<[u8]> = std::slice::from_raw_parts(data as *const u8, len).into();
        store.insert(key.0, kind, bytes);
        raw::GIT_OK
    }
}

extern "C" fn odb_exists(backend: *mut raw::git_odb_backend, oid: *const raw::git_oid) -> c_int {
    unsafe {
        let store = backend_store(backend);
        c_int::from(store.map.contains_sync(&oid_to_key(oid)))
    }
}

extern "C" fn odb_free(backend: *mut raw::git_odb_backend) {
    unsafe {
        drop(Box::from_raw(backend as *mut JoshBackend));
    }
}
