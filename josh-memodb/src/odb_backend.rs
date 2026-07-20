//! Generic scaffolding for a custom libgit2 ODB backend.
//!
//! [`OdbBackend`] is a safe Rust trait; [`register`] lifts any implementation into a
//! `git_odb_backend` and installs it on a repository. The trait shape mirrors the `git2-rs` fork
//! (branch `metahead/odb-backends`), where a safe trait object is wrapped by a `#[repr(C)]`
//! [`RawOdbBackend`] and called through `extern "C"` trampolines.
//!
//! Rather than registering the backend *alongside* the repository's on-disk loose/pack backends,
//! [`register`] *replaces* the repository's entire ODB with a new one containing only this backend,
//! which delegates read-side misses to the original on-disk ODB (kept as [`RawOdbBackend::delegate`]).
//! The reason is `git_odb_write`: it unconditionally calls `git_odb__freshen` before every write,
//! and freshen walks the ODB's backends doing a per-object filesystem stat/touch on the loose
//! backend. With the loose/pack backends absent from the repo's ODB and a memory-only `freshen`
//! callback on this backend, that wasted disk I/O disappears from the filter hot path while reads
//! still resolve on-disk objects via delegation.
//!
//! git2 does not expose the raw `git_odb`/`git_repository` pointers (its `Binding` trait is
//! private), so [`register`] reads the pointer out of the single-field `Repository` newtype. That
//! cast is sound only while josh pins `git2` exactly and must be re-verified on every upgrade, along
//! with the continued presence of `git_odb_new`/`git_repository_set_odb`/`git_odb_object_*`.

use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::Arc;

use crate::odb_cache::{CacheObjectData, ObjectCache};
use git2::{ErrorCode, ObjectType, Oid};
use libgit2_sys as raw;

/// A safe Rust interface to a libgit2 object-database backend.
///
/// Any implementation is turned into a `git_odb_backend` by [`register`]. Methods receive
/// [`git2::Oid`] / [`git2::ObjectType`] and return [`git2::Error`] so any libgit2 failure code can
/// be signalled: signal "not present here" with [`git2::ErrorCode::NotFound`], which the trampoline
/// forwards to the on-disk delegate. Any other error code propagates and aborts the lookup.
pub trait OdbBackend {
    fn read_header(&self, oid: Oid) -> Result<(usize, ObjectType), git2::Error>;
    fn read(&self, oid: Oid) -> Result<(Arc<[u8]>, ObjectType), git2::Error>;
    /// A duplicate (content-addressed) `oid` may be treated as a no-op.
    fn write(&mut self, oid: Oid, data: &[u8], kind: ObjectType) -> Result<(), git2::Error>;
    fn exists(&self, oid: Oid) -> bool;
    /// Resolve an abbreviated object id to its full [`Oid`], if this backend holds a unique match.
    fn exists_prefix(&self, oid: Oid, oid_len: usize) -> Option<Oid>;
}

/// `#[repr(C)]` wrapper placing a [`raw::git_odb_backend`] first (so a `*mut git_odb_backend`
/// handed to a trampoline casts back to this) followed by the owning trait object. libgit2 owns the
/// allocation and releases it through the `free` trampoline.
#[repr(C)]
struct RawOdbBackend {
    raw: raw::git_odb_backend,
    obj: Box<dyn OdbBackend>,
    cache: ObjectCache,
    /// The repository's original on-disk ODB (loose + pack), kept as an owned reference so read-side
    /// callbacks can delegate to it on a memory miss. Released in [`backend_free`].
    delegate: *mut raw::git_odb,
}

impl RawOdbBackend {
    /// Cast a libgit2 backend pointer back to its wrapper. Safe only because the pointer always
    /// originates from [`new`], which allocates a `RawOdbBackend` and hands libgit2 its address.
    unsafe fn from_raw(backend: *mut raw::git_odb_backend) -> &'static mut RawOdbBackend {
        unsafe { &mut *(backend as *mut RawOdbBackend) }
    }

    extern "C" fn backend_read(
        data_p: *mut *mut c_void,
        len_p: *mut usize,
        kind_p: *mut raw::git_object_t,
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
    ) -> c_int {
        let (wrapper, roid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        if let Some((kind, data)) = wrapper.cache.load(unsafe { *oid }) {
            unsafe {
                let len = data.len();
                let buf = raw::git_odb_backend_data_alloc(backend, len);
                if buf.is_null() {
                    return raw::GIT_ERROR;
                }
                ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, len);
                *data_p = buf;
                *len_p = len;
                *kind_p = kind;
                return raw::GIT_OK;
            }
        }

        match wrapper.obj.read(roid) {
            Ok((data, kind)) => {
                let kind_raw = kind.raw();
                let len = data.len();

                // Copy into libgit2's buffer first (unavoidable: libgit2 owns that allocation),
                // then hand the bytes we already own to the cache.
                unsafe {
                    let buf = raw::git_odb_backend_data_alloc(backend, len);
                    if buf.is_null() {
                        return raw::GIT_ERROR;
                    }
                    ptr::copy_nonoverlapping(data.as_ref().as_ptr(), buf as *mut u8, len);
                    *data_p = buf;
                    *len_p = len;
                    *kind_p = kind_raw;
                }

                // `data` is an `Arc<[u8]>` shared with the in-memory store, so moving it into the
                // cache is a refcount bump — mem_odb and the cache back the same allocation.
                wrapper
                    .cache
                    .store(unsafe { *oid }, kind_raw, CacheObjectData::Allocated(data));

                raw::GIT_OK
            }
            Err(err) if is_not_found(&err) && !wrapper.delegate.is_null() => {
                // Memory miss: read through to the original on-disk ODB and copy its bytes into a
                // backend-owned buffer (libgit2 frees `*data_p` via the backend's allocator).
                let obj = unsafe {
                    let mut obj: *mut raw::git_odb_object = ptr::null_mut();
                    let rc = raw::git_odb_read(&mut obj, wrapper.delegate, oid);
                    if rc != 0 {
                        return rc;
                    }
                    obj
                };

                let (src, len, kind) = unsafe {
                    let src = raw::git_odb_object_data(obj) as *const u8;
                    let len = raw::git_odb_object_size(obj);
                    let kind = raw::git_odb_object_type(obj);
                    (src, len, kind)
                };

                let data_slice = unsafe { std::slice::from_raw_parts(src, len) };
                wrapper
                    .cache
                    .store(unsafe { *oid }, kind, CacheObjectData::Ref(data_slice));

                unsafe {
                    let buf = raw::git_odb_backend_data_alloc(backend, len);
                    if buf.is_null() {
                        raw::git_odb_object_free(obj);
                        return raw::GIT_ERROR;
                    }
                    ptr::copy_nonoverlapping(src, buf as *mut u8, len);
                    *data_p = buf;
                    *len_p = len;
                    *kind_p = kind;
                    raw::git_odb_object_free(obj);
                    raw::GIT_OK
                }
            }
            Err(err) => err.raw_code(),
        }
    }

    extern "C" fn backend_read_header(
        len_p: *mut usize,
        kind_p: *mut raw::git_object_t,
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
    ) -> c_int {
        let (wrapper, roid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        match wrapper.obj.read_header(roid) {
            Ok((len, kind)) => unsafe {
                *len_p = len;
                *kind_p = kind.raw();
                raw::GIT_OK
            },
            Err(err) if is_not_found(&err) && !wrapper.delegate.is_null() => unsafe {
                raw::git_odb_read_header(len_p, kind_p, wrapper.delegate, oid)
            },
            Err(err) => err.raw_code(),
        }
    }

    extern "C" fn backend_write(
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
        data: *const c_void,
        len: usize,
        kind: raw::git_object_t,
    ) -> c_int {
        let (wrapper, oid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        let kind = match ObjectType::from_raw(kind) {
            Some(kind) => kind,
            None => return raw::GIT_ERROR,
        };
        let data = unsafe { std::slice::from_raw_parts(data as *const u8, len) };
        match wrapper.obj.write(oid, data, kind) {
            Ok(()) => raw::GIT_OK,
            Err(err) => err.raw_code(),
        }
    }

    extern "C" fn backend_exists(
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
    ) -> c_int {
        let (wrapper, roid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        if wrapper.obj.exists(roid) {
            return 1;
        }
        if wrapper.delegate.is_null() {
            return 0;
        }
        unsafe { raw::git_odb_exists(wrapper.delegate, oid) }
    }

    extern "C" fn backend_exists_prefix(
        out_oid: *mut raw::git_oid,
        backend: *mut raw::git_odb_backend,
        oid_prefix: *const raw::git_oid,
        len: usize,
    ) -> c_int {
        let (wrapper, roid) =
            unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid_prefix)) };
        match wrapper.obj.exists_prefix(roid, len) {
            Some(oid) => unsafe {
                (*out_oid).id.copy_from_slice(oid.as_bytes());
                raw::GIT_OK
            },
            None if !wrapper.delegate.is_null() => unsafe {
                raw::git_odb_exists_prefix(out_oid, wrapper.delegate, oid_prefix, len)
            },
            None => raw::GIT_ENOTFOUND,
        }
    }

    /// Object enumeration is delegated to the on-disk ODB only; in-memory objects are not enumerated
    /// (the filter path inserts objects into the packbuilder by OID, not via `foreach`).
    extern "C" fn backend_foreach(
        backend: *mut raw::git_odb_backend,
        cb: raw::git_odb_foreach_cb,
        payload: *mut c_void,
    ) -> c_int {
        let wrapper = unsafe { RawOdbBackend::from_raw(backend) };
        if wrapper.delegate.is_null() {
            return raw::GIT_OK;
        }
        unsafe { raw::git_odb_foreach(wrapper.delegate, cb, payload) }
    }

    /// Memory-only freshen, used by `git_odb__freshen` on every `git_odb_write`. Returns `GIT_OK`
    /// when the object is already in memory (so the write is skipped — in-run dedup) and
    /// `GIT_ENOTFOUND` otherwise (so the write proceeds to `backend_write`). It deliberately never
    /// consults the on-disk delegate: avoiding that per-object filesystem stat/touch is the whole
    /// point of the ODB swap. Objects already on disk are deduplicated at the flush boundary instead
    /// (see [`filter_absent_on_disk`]).
    extern "C" fn backend_freshen(
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
    ) -> c_int {
        let (wrapper, roid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        if wrapper.obj.exists(roid) {
            raw::GIT_OK
        } else {
            raw::GIT_ENOTFOUND
        }
    }

    extern "C" fn backend_free(backend: *mut raw::git_odb_backend) {
        // libgit2 is done with the backend; release the delegate ODB reference and reclaim the
        // wrapper allocation.
        unsafe {
            let be = Box::from_raw(backend as *mut RawOdbBackend);
            if !be.delegate.is_null() {
                raw::git_odb_free(be.delegate);
            }
            drop(be);
        }
    }
}

fn is_not_found(err: &git2::Error) -> bool {
    err.code() == ErrorCode::NotFound
}

unsafe fn oid_from_raw(oid: *const raw::git_oid) -> Oid {
    Oid::from_bytes(unsafe { &(*oid).id }).expect("git_oid is 20 raw bytes")
}

/// Build a `git_odb_backend` wrapping `callee` and delegating read-side misses to `delegate`. The
/// returned pointer is handed to libgit2 (which frees it via the `free` trampoline); the caller must
/// not free it on success.
fn new(
    odb: *mut raw::git_odb,
    delegate: *mut raw::git_odb,
    cache_limit: Option<usize>,
    callee: impl OdbBackend + 'static,
) -> *mut raw::git_odb_backend {
    let backend = raw::git_odb_backend {
        version: raw::GIT_ODB_BACKEND_VERSION,
        odb,
        read: Some(RawOdbBackend::backend_read),
        read_prefix: None,
        read_header: Some(RawOdbBackend::backend_read_header),
        write: Some(RawOdbBackend::backend_write),
        writestream: None,
        readstream: None,
        exists: Some(RawOdbBackend::backend_exists),
        exists_prefix: Some(RawOdbBackend::backend_exists_prefix),
        refresh: None,
        foreach: Some(RawOdbBackend::backend_foreach),
        writepack: None,
        writemidx: None,
        freshen: Some(RawOdbBackend::backend_freshen),
        free: Some(RawOdbBackend::backend_free),
    };
    let wrapper = RawOdbBackend {
        raw: backend,
        cache: ObjectCache::new(cache_limit),
        obj: Box::new(callee),
        delegate,
    };
    Box::into_raw(Box::new(wrapper)) as *mut raw::git_odb_backend
}

/// Replace `repo`'s object database with one containing only `backend`, which delegates read-side
/// misses to the original on-disk ODB. Writes then land in `backend` and `git_odb__freshen` never
/// touches the filesystem (see the module docs).
///
/// Must be called at most once per `git2::Repository` handle (josh opens a fresh handle per
/// transaction and per overflow flush); calling it twice would swap an already-swapped ODB.
///
/// git2 does not expose the raw `git_repository` pointer, so it is read out of the single-field
/// `Repository` newtype. This is sound only while josh pins `git2` exactly; re-verify the layout on
/// every upgrade.
pub fn register<B>(
    repo: &git2::Repository,
    backend: B,
    cache_limit: Option<usize>,
) -> Result<(), git2::Error>
where
    B: OdbBackend + 'static,
{
    unsafe {
        let repo_raw = *(repo as *const git2::Repository as *const *mut raw::git_repository);

        // Owned (refcount-incremented) reference to the current on-disk ODB; handed to the backend
        // as its read delegate and released in `backend_free`.
        let mut old: *mut raw::git_odb = ptr::null_mut();
        if raw::git_repository_odb(&mut old, repo_raw) != raw::GIT_OK {
            return Err(git2::Error::last_error(raw::GIT_ERROR));
        }

        let mut new: *mut raw::git_odb = ptr::null_mut();
        if raw::git_odb_new(&mut new) != raw::GIT_OK {
            raw::git_odb_free(old);
            return Err(git2::Error::last_error(raw::GIT_ERROR));
        }

        let backend = self::new(new, old, cache_limit, backend);
        if raw::git_odb_add_backend(new, backend, 1000) != raw::GIT_OK {
            // The backend was not adopted by `new`; reclaim it. Its Drop does not touch the raw
            // `delegate` pointer, so free `old` explicitly.
            drop(Box::from_raw(backend as *mut RawOdbBackend));
            raw::git_odb_free(old);
            raw::git_odb_free(new);
            return Err(git2::Error::last_error(raw::GIT_ERROR));
        }

        if raw::git_repository_set_odb(repo_raw, new) != raw::GIT_OK {
            // The repo kept its original ODB. Dropping our only reference to `new` frees it, which
            // runs `backend_free` and releases `old`.
            raw::git_odb_free(new);
            return Err(git2::Error::last_error(raw::GIT_ERROR));
        }

        // The repo took its own reference on `new`; drop our local one. `old` stays alive through
        // the backend's `delegate` field until the repo (and thus `new`) is freed.
        raw::git_odb_free(new);
        Ok(())
    }
}

/// The on-disk ODB that `repo`'s swapped in-memory backend delegates reads to, or null if the
/// backend is not installed. Reaches the backend at index 0 of the (swapped) repo ODB, which is
/// always the single backend added by [`register`].
unsafe fn delegate_of(repo: &git2::Repository) -> *mut raw::git_odb {
    unsafe {
        let repo_raw = *(repo as *const git2::Repository as *const *mut raw::git_repository);
        let mut odb: *mut raw::git_odb = ptr::null_mut();
        if raw::git_repository_odb(&mut odb, repo_raw) != raw::GIT_OK {
            return ptr::null_mut();
        }
        let mut backend: *mut raw::git_odb_backend = ptr::null_mut();
        let rc = raw::git_odb_get_backend(&mut backend, odb, 0);
        raw::git_odb_free(odb);
        if rc != raw::GIT_OK || backend.is_null() {
            return ptr::null_mut();
        }
        (*(backend as *const RawOdbBackend)).delegate
    }
}

/// Return the subset of `oids` that are NOT already present in the on-disk ODB that `repo`'s swapped
/// backend delegates to, using `NO_REFRESH` (matching the non-refreshing semantics of the write-time
/// freshen that the memory-only [`RawOdbBackend::backend_freshen`] replaced). The memory-only
/// freshen no longer deduplicates writes against disk, so a flush uses this to pack only
/// genuinely-new objects and keep the on-disk layout deterministic. If the backend is not installed
/// (no delegate), every oid is returned.
pub fn filter_absent_on_disk(repo: &git2::Repository, oids: &[Oid]) -> Vec<Oid> {
    unsafe {
        let disk = delegate_of(repo);
        if disk.is_null() {
            return oids.to_vec();
        }
        oids.iter()
            .copied()
            .filter(|oid| {
                let mut goid: raw::git_oid = std::mem::zeroed();
                goid.id.copy_from_slice(oid.as_bytes());
                let flags = raw::GIT_ODB_LOOKUP_NO_REFRESH as std::os::raw::c_uint;
                raw::git_odb_exists_ext(disk, &goid, flags) == 0
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal in-memory backend used to exercise the trampolines end-to-end through libgit2.
    struct MapBackend {
        data: std::collections::HashMap<Oid, (Arc<[u8]>, ObjectType)>,
    }

    impl OdbBackend for MapBackend {
        fn read_header(&self, oid: Oid) -> Result<(usize, ObjectType), git2::Error> {
            self.read(oid).map(|(data, kind)| (data.len(), kind))
        }

        fn read(&self, oid: Oid) -> Result<(Arc<[u8]>, ObjectType), git2::Error> {
            self.data
                .get(&oid)
                .map(|(data, kind)| (data.clone(), *kind))
                .ok_or_else(not_found)
        }

        fn write(&mut self, oid: Oid, data: &[u8], kind: ObjectType) -> Result<(), git2::Error> {
            self.data
                .entry(oid)
                .or_insert_with(|| (Arc::from(data), kind));
            Ok(())
        }

        fn exists(&self, oid: Oid) -> bool {
            self.data.contains_key(&oid)
        }

        fn exists_prefix(&self, oid: Oid, oid_len: usize) -> Option<Oid> {
            self.data
                .keys()
                .find(|key| key.as_bytes()[..oid_len] == oid.as_bytes()[..oid_len])
                .copied()
        }
    }

    fn not_found() -> git2::Error {
        git2::Error::new(
            git2::ErrorCode::NotFound,
            git2::ErrorClass::Odb,
            "not in test backend",
        )
    }

    /// Write a blob through a repo whose odb has been swapped for a `MapBackend`, then read it back.
    /// libgit2 must route both the write and the read through our trampolines, and the delegate
    /// fall-through path must keep working for objects the backend does not hold.
    #[test]
    fn round_trip_through_registered_backend() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        let backend = MapBackend {
            data: std::collections::HashMap::new(),
        };
        register(&repo, backend, None).unwrap();

        let payload = b"hello, in-memory odb";
        let id = repo.blob(payload).unwrap();

        // The blob is only in the backend, not on disk; a read must come back through `read`.
        let blob = repo.find_blob(id).unwrap();
        assert_eq!(blob.content(), payload);

        assert!(repo.odb().unwrap().exists(id));

        // An unknown oid must yield NotFound so lookups fall through rather than hard-failing.
        let unknown = Oid::from_str("0000000000000000000000000000000000000001").unwrap();
        assert!(!repo.odb().unwrap().exists(unknown));
    }
}
