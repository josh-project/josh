//! In-memory git object database backend.
//!
//! josh produces enormous numbers of git objects while filtering, and writing each as a loose
//! object (a separate file + zlib stream + directory insert) dominates the cost of large rewrites.
//! This module installs a custom libgit2 ODB backend that keeps written objects in a process-global
//! concurrent map and serves read-back from memory. Objects are flushed to packfiles separately.
//!
//! Rather than registering the backend *alongside* the on-disk loose/pack backends, every
//! repository josh opens has its entire ODB *replaced* by a new one containing only this backend
//! (see [`register`]). The backend delegates read-side misses to the original on-disk ODB. The
//! reason is `git_odb_write`: it unconditionally calls `git_odb__freshen` before every write, and
//! freshen walks the ODB's backends doing a per-object filesystem stat/touch on the loose backend.
//! With the loose/pack backends absent from the repo's ODB and a memory-only `freshen` callback on
//! this backend, that wasted disk I/O disappears from the filter hot path while reads still resolve
//! on-disk objects via delegation.

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

    // The in-memory backend's `freshen` is deliberately memory-only (so the hot write path never
    // stats disk), which means objects already present on disk get re-written into memory rather
    // than deduplicated. Dedup them here instead — once per unique object at this flush boundary,
    // not per write — so we only pack genuinely-new objects and avoid redundant packfiles.
    let disk = unsafe { repo_delegate_odb(repo) };

    let mut pb = repo.packbuilder()?;
    let mut packed = 0usize;
    for oid in &oids {
        if !disk.is_null() && unsafe { disk_contains(disk, oid) } {
            continue;
        }
        pb.insert_object(*oid, None)?;
        packed += 1;
    }
    if packed > 0 {
        pb.write(&repo.path().join("objects").join("pack"), 0)?;
    }

    // Every flushed object is now durable (already on disk, or just packed), so evict the lot.
    for oid in &oids {
        STORE.map.remove_sync(&Key(*oid));
    }
    Ok(())
}

/// The on-disk (loose + pack) ODB that this repo's in-memory backend delegates reads to, or null if
/// the backend is not installed. Reaches the backend at index 0 of the (swapped) repo ODB, which is
/// always our single [`JoshBackend`].
unsafe fn repo_delegate_odb(repo: &git2::Repository) -> *mut raw::git_odb {
    unsafe {
        let repo_raw = *(repo as *const git2::Repository as *const *mut raw::git_repository);
        let mut odb: *mut raw::git_odb = std::ptr::null_mut();
        if raw::git_repository_odb(&mut odb, repo_raw) != 0 {
            return std::ptr::null_mut();
        }
        let mut backend: *mut raw::git_odb_backend = std::ptr::null_mut();
        let rc = raw::git_odb_get_backend(&mut backend, odb, 0);
        raw::git_odb_free(odb);
        if rc != 0 || backend.is_null() {
            return std::ptr::null_mut();
        }
        backend_delegate(backend)
    }
}

unsafe fn disk_contains(disk: *mut raw::git_odb, oid: &git2::Oid) -> bool {
    unsafe {
        let mut goid: raw::git_oid = std::mem::zeroed();
        goid.id.copy_from_slice(oid.as_bytes());
        // NO_REFRESH: don't re-scan the objects dir for newly-appeared packs. This matches the
        // semantics of the write-time `git_odb__freshen` we replaced (which never refreshes), so the
        // set of objects packed here is identical to before — important for the proxy's
        // deterministic on-disk layout.
        let flags = raw::GIT_ODB_LOOKUP_NO_REFRESH as std::os::raw::c_uint;
        raw::git_odb_exists_ext(disk, &goid, flags) != 0
    }
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
    /// The repository's original on-disk ODB (loose + pack), kept as an owned reference so read-side
    /// callbacks can delegate to it on a memory miss. Released in [`odb_free`].
    delegate: *mut raw::git_odb,
}

/// Replace `repo`'s object database with one containing only the in-memory backend, which delegates
/// read-side misses to the original on-disk ODB. Writes then land in memory and `git_odb__freshen`
/// never touches the filesystem (see the module docs).
///
/// Must be called at most once per `git2::Repository` handle (josh opens a fresh handle per
/// transaction and per `flush_all_at`); calling it twice would swap an already-swapped ODB.
///
/// git2 does not expose the raw `git_odb`/`git_repository` pointers (its `Binding` trait lives in a
/// private module), so we read the pointer out of the single-field `Repository` newtype. This is
/// sound only because josh pins `git2` exactly; a version bump must re-verify the layout and the
/// continued presence of `git_odb_new`/`git_repository_set_odb`/`git_odb_object_*`.
pub(crate) fn register(repo: &git2::Repository) {
    unsafe {
        let repo_raw = *(repo as *const git2::Repository as *const *mut raw::git_repository);

        // Owned (refcount-incremented) reference to the current on-disk ODB; handed to the backend
        // as its read delegate and released in `odb_free`.
        let mut old: *mut raw::git_odb = std::ptr::null_mut();
        if raw::git_repository_odb(&mut old, repo_raw) != 0 {
            return;
        }

        let mut new: *mut raw::git_odb = std::ptr::null_mut();
        if raw::git_odb_new(&mut new) != 0 {
            raw::git_odb_free(old);
            return;
        }

        let backend = new_backend(old);
        if raw::git_odb_add_backend(new, backend, 1000) != 0 {
            // The backend was not adopted by `new`; reclaim it. Its Drop does not touch the raw
            // `delegate` pointer, so free `old` explicitly.
            drop(Box::from_raw(backend as *mut JoshBackend));
            raw::git_odb_free(old);
            raw::git_odb_free(new);
            return;
        }

        if raw::git_repository_set_odb(repo_raw, new) != 0 {
            // The repo kept its original ODB. Dropping our only reference to `new` frees it, which
            // runs `odb_free` and releases `old`.
            raw::git_odb_free(new);
            return;
        }

        // The repo took its own reference on `new`; drop our local one. `old` stays alive through
        // the backend's `delegate` field until the repo (and thus `new`) is freed.
        raw::git_odb_free(new);
    }
}

fn new_backend(delegate: *mut raw::git_odb) -> *mut raw::git_odb_backend {
    let mut be = Box::new(JoshBackend {
        // All-zero is a valid git_odb_backend: null odb pointer and `None` for every callback.
        base: unsafe { std::mem::zeroed() },
        store: STORE.clone(),
        delegate,
    });
    unsafe {
        raw::git_odb_init_backend(&mut be.base, raw::GIT_ODB_BACKEND_VERSION);
    }
    be.base.read = Some(odb_read);
    be.base.read_header = Some(odb_read_header);
    be.base.write = Some(odb_write);
    be.base.exists = Some(odb_exists);
    be.base.exists_prefix = Some(odb_exists_prefix);
    be.base.foreach = Some(odb_foreach);
    be.base.freshen = Some(odb_freshen);
    be.base.free = Some(odb_free);
    // `read_prefix` is intentionally left unset: libgit2-sys exposes no `git_odb_read_prefix` to
    // delegate to, and the filter path resolves only full 20-byte OIDs.
    Box::into_raw(be) as *mut raw::git_odb_backend
}

unsafe fn backend_store<'a>(backend: *mut raw::git_odb_backend) -> &'a MemOdb {
    unsafe { &(*(backend as *const JoshBackend)).store }
}

unsafe fn backend_delegate(backend: *mut raw::git_odb_backend) -> *mut raw::git_odb {
    unsafe { (*(backend as *const JoshBackend)).delegate }
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
            None => {
                // Memory miss: read through to the original on-disk ODB and copy its bytes into a
                // backend-owned buffer (libgit2 frees `*buffer_p` via the backend's allocator).
                let delegate = backend_delegate(backend);
                if delegate.is_null() {
                    return raw::GIT_ENOTFOUND;
                }
                let mut obj: *mut raw::git_odb_object = std::ptr::null_mut();
                let rc = raw::git_odb_read(&mut obj, delegate, oid);
                if rc != 0 {
                    return rc;
                }
                let src = raw::git_odb_object_data(obj) as *const u8;
                let len = raw::git_odb_object_size(obj);
                let kind = raw::git_odb_object_type(obj);
                let buf = raw::git_odb_backend_data_alloc(backend, len);
                if buf.is_null() {
                    raw::git_odb_object_free(obj);
                    return raw::GIT_ERROR;
                }
                std::ptr::copy_nonoverlapping(src, buf as *mut u8, len);
                *buffer_p = buf;
                *len_p = len;
                *type_p = kind;
                raw::git_odb_object_free(obj);
                raw::GIT_OK
            }
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
            None => {
                let delegate = backend_delegate(backend);
                if delegate.is_null() {
                    return raw::GIT_ENOTFOUND;
                }
                raw::git_odb_read_header(len_p, type_p, delegate, oid)
            }
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
        if store.map.contains_sync(&oid_to_key(oid)) {
            return 1;
        }
        let delegate = backend_delegate(backend);
        if delegate.is_null() {
            return 0;
        }
        raw::git_odb_exists(delegate, oid)
    }
}

/// Memory-only freshen, used by `git_odb__freshen` on every `git_odb_write`. Returns `GIT_OK` when
/// the object is already in memory (so the write is skipped — in-run dedup) and `GIT_ENOTFOUND`
/// otherwise (so the write proceeds to `odb_write`). It deliberately never consults the on-disk
/// delegate: avoiding that per-object filesystem stat/touch is the whole point of this module.
extern "C" fn odb_freshen(backend: *mut raw::git_odb_backend, oid: *const raw::git_oid) -> c_int {
    unsafe {
        let store = backend_store(backend);
        if store.map.contains_sync(&oid_to_key(oid)) {
            raw::GIT_OK
        } else {
            raw::GIT_ENOTFOUND
        }
    }
}

/// Short-OID lookups are delegated to the on-disk ODB only; in-memory objects are not matched by
/// prefix. The filter path resolves only full OIDs, so this is insurance for incidental libgit2 use.
extern "C" fn odb_exists_prefix(
    out: *mut raw::git_oid,
    backend: *mut raw::git_odb_backend,
    short_oid: *const raw::git_oid,
    len: usize,
) -> c_int {
    unsafe {
        let delegate = backend_delegate(backend);
        if delegate.is_null() {
            return raw::GIT_ENOTFOUND;
        }
        raw::git_odb_exists_prefix(out, delegate, short_oid, len)
    }
}

/// Object enumeration is delegated to the on-disk ODB only; in-memory objects are not enumerated.
/// Unused on the filter path (the flush inserts objects into the packbuilder by OID, not via
/// `foreach`).
extern "C" fn odb_foreach(
    backend: *mut raw::git_odb_backend,
    cb: raw::git_odb_foreach_cb,
    payload: *mut c_void,
) -> c_int {
    unsafe {
        let delegate = backend_delegate(backend);
        if delegate.is_null() {
            return raw::GIT_OK;
        }
        raw::git_odb_foreach(delegate, cb, payload)
    }
}

extern "C" fn odb_free(backend: *mut raw::git_odb_backend) {
    unsafe {
        let be = Box::from_raw(backend as *mut JoshBackend);
        if !be.delegate.is_null() {
            raw::git_odb_free(be.delegate);
        }
        drop(be);
    }
}
