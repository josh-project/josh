//! josh's object store.
//!
//! [`MemOdb`] is a per-operation in-memory store that buffers the objects josh produces and
//! flushes them to a packfile at transaction and external-git boundaries. [`Odb`] is the facade
//! every reader and writer goes through: memory first, then the repository's objects on disk.

mod flusher;
pub mod hash;
pub mod mem_odb;
pub mod odb;
pub mod pack;

pub use hash::PassthroughHasher;
pub use mem_odb::MemOdb;
pub use odb::{Bytes, Odb};
pub use pack::objects_dir;
