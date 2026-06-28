//! Custom libgit2 object-database backends for josh.
//!
//! [`OdbBackend`] is a safe Rust trait that [`odb_backend::register`] lifts into a
//! `git_odb_backend` registered on a repository, so the usual `repo.blob()` / treebuilder /
//! commit calls route through it with no call-site changes. [`MemOdb`] is the concrete backend
//! josh uses: a per-operation in-memory store that buffers filtered objects and flushes them to a
//! packfile at transaction and external-git boundaries.

pub mod hash;
pub mod mem_odb;
mod odb_backend;
pub mod pack;

pub use hash::PassthroughHasher;
pub use mem_odb::MemOdb;
pub use odb_backend::{OdbBackend, register};
