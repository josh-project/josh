//! Generic scaffolding for a custom libgit2 ODB backend.
//!
//! [`OdbBackend`] is a safe Rust trait; [`register`] lifts any implementation into a
//! `git_odb_backend` registered on a repository's object database. The trait shape mirrors the
//! `git2-rs` fork (branch `metahead/odb-backends`), where a safe trait object is wrapped by a
//! `#[repr(C)]` [`RawOdbBackend`] and called through `extern "C"` trampolines.
//!
//! git2 does not expose the raw `git_odb`/`git_repository` pointers (its `Binding` trait is
//! private), so [`register`] reads the pointer out of the single-field `Repository` newtype. That
//! cast is sound only while josh pins `git2` exactly and must be re-verified on every upgrade.

use std::ffi::{c_int, c_void};
use std::ptr;

use git2::{ObjectType, Oid};
use libgit2_sys as raw;

/// A safe Rust interface to a libgit2 object-database backend.
///
/// Any implementation is turned into a `git_odb_backend` by [`register`]. Methods receive
/// [`git2::Oid`] / [`git2::ObjectType`] and return [`git2::Error`] so any libgit2 failure code can
/// be signalled: signal "not present here" with [`git2::ErrorCode::NotFound`], which the trampoline
/// forwards as `GIT_ENOTFOUND` so libgit2 falls through to the next backend (e.g. the on-disk
/// pack/loose backends). Any other error code propagates and aborts the lookup.
pub trait OdbBackend {
    fn read_header(&self, oid: Oid) -> Result<(usize, ObjectType), git2::Error>;
    fn read(&self, oid: Oid) -> Result<(Vec<u8>, ObjectType), git2::Error>;
    /// A duplicate (content-addressed) `oid` may be treated as a no-op.
    fn write(&mut self, oid: Oid, data: Vec<u8>, kind: ObjectType) -> Result<(), git2::Error>;
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
        let (wrapper, oid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        match wrapper.obj.read(oid) {
            Ok((data, kind)) => unsafe {
                let len = data.len();
                let buf = raw::git_odb_backend_data_alloc(backend, len);
                if buf.is_null() {
                    return raw::GIT_ERROR;
                }
                ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, len);
                *data_p = buf;
                *len_p = len;
                *kind_p = kind.raw();
                raw::GIT_OK
            },
            Err(err) => err.raw_code(),
        }
    }

    extern "C" fn backend_read_header(
        len_p: *mut usize,
        kind_p: *mut raw::git_object_t,
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
    ) -> c_int {
        let (wrapper, oid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        match wrapper.obj.read_header(oid) {
            Ok((len, kind)) => unsafe {
                *len_p = len;
                *kind_p = kind.raw();
                raw::GIT_OK
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
        let data = unsafe { std::slice::from_raw_parts(data as *const u8, len) }.to_vec();
        match wrapper.obj.write(oid, data, kind) {
            Ok(()) => raw::GIT_OK,
            Err(err) => err.raw_code(),
        }
    }

    extern "C" fn backend_exists(
        backend: *mut raw::git_odb_backend,
        oid: *const raw::git_oid,
    ) -> c_int {
        let (wrapper, oid) = unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid)) };
        c_int::from(wrapper.obj.exists(oid))
    }

    extern "C" fn backend_exists_prefix(
        out_oid: *mut raw::git_oid,
        backend: *mut raw::git_odb_backend,
        oid_prefix: *const raw::git_oid,
        len: usize,
    ) -> c_int {
        let (wrapper, oid_prefix) =
            unsafe { (RawOdbBackend::from_raw(backend), oid_from_raw(oid_prefix)) };
        match wrapper.obj.exists_prefix(oid_prefix, len) {
            Some(oid) => unsafe {
                (*out_oid).id.copy_from_slice(oid.as_bytes());
                raw::GIT_OK
            },
            None => raw::GIT_ENOTFOUND,
        }
    }

    extern "C" fn backend_free(backend: *mut raw::git_odb_backend) {
        // libgit2 is done with the backend; reclaim the wrapper allocation.
        unsafe {
            drop(Box::from_raw(backend as *mut RawOdbBackend));
        }
    }
}

unsafe fn oid_from_raw(oid: *const raw::git_oid) -> Oid {
    Oid::from_bytes(unsafe { &(*oid).id }).expect("git_oid is 20 raw bytes")
}

/// Build a `git_odb_backend` wrapping `callee`, owned by `odb`. The returned pointer is handed to
/// libgit2 (which frees it via the `free` trampoline); the caller must not free it on success.
fn new(odb: *mut raw::git_odb, callee: impl OdbBackend + 'static) -> *mut raw::git_odb_backend {
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
        foreach: None,
        writepack: None,
        writemidx: None,
        freshen: None,
        free: Some(RawOdbBackend::backend_free),
    };
    let wrapper = RawOdbBackend {
        raw: backend,
        obj: Box::new(callee),
    };
    Box::into_raw(Box::new(wrapper)) as *mut raw::git_odb_backend
}

/// Register `backend` on `repo`'s object database at `priority`. Higher values take precedence
/// over the on-disk loose (2) and pack (1) backends.
///
/// git2 does not expose the raw `git_repository` pointer, so it is read out of the single-field
/// `Repository` newtype. This is sound only while josh pins `git2` exactly; re-verify the layout on
/// every upgrade.
pub fn register<B>(repo: &git2::Repository, priority: i32, backend: B) -> Result<(), git2::Error>
where
    B: OdbBackend + 'static,
{
    unsafe {
        let repo_raw = *(repo as *const git2::Repository as *const *mut raw::git_repository);
        let mut odb: *mut raw::git_odb = ptr::null_mut();
        if raw::git_repository_odb(&mut odb, repo_raw) != raw::GIT_OK {
            return Err(git2::Error::last_error(raw::GIT_ERROR));
        }
        let backend = new(odb, backend);
        if raw::git_odb_add_backend(odb, backend, priority) != raw::GIT_OK {
            // add_backend failed: reclaim the allocation to avoid leaking it.
            drop(Box::from_raw(backend as *mut RawOdbBackend));
            return Err(git2::Error::last_error(raw::GIT_ERROR));
        }
        // git_repository_odb returned an owned (refcounted) handle; the odb itself is still held by
        // the repo, which now also owns the backend (freed via `free` when the repo drops).
        raw::git_odb_free(odb);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal in-memory backend used to exercise the trampolines end-to-end through libgit2.
    struct MapBackend {
        data: std::collections::HashMap<Oid, (Vec<u8>, ObjectType)>,
    }

    impl OdbBackend for MapBackend {
        fn read_header(&self, oid: Oid) -> Result<(usize, ObjectType), git2::Error> {
            self.read(oid).map(|(data, kind)| (data.len(), kind))
        }

        fn read(&self, oid: Oid) -> Result<(Vec<u8>, ObjectType), git2::Error> {
            self.data.get(&oid).cloned().ok_or_else(not_found)
        }

        fn write(&mut self, oid: Oid, data: Vec<u8>, kind: ObjectType) -> Result<(), git2::Error> {
            self.data.insert(oid, (data, kind));
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

    /// Write a blob through a repo whose odb has a `MapBackend` registered at high priority, then
    /// read it back. libgit2 must route both the write and the read through our trampolines, and the
    /// fall-through path must keep working for objects the backend does not hold.
    #[test]
    fn round_trip_through_registered_backend() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        let backend = MapBackend {
            data: std::collections::HashMap::new(),
        };
        register(&repo, 1000, backend).unwrap();

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
