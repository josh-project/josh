//! josh's object store.
//!
//! [`MemOdb`] is an in-memory store that buffers the objects josh produces and packs them to disk
//! on its own schedule. There is one per objects directory (see [`registry`]), so it outlives the
//! transactions writing to it. [`Odb`] is the facade every reader and writer goes through: memory
//! first, then the repository's objects on disk.

mod flusher;
pub mod hash;
pub mod mem_odb;
pub mod odb;
pub mod pack;
pub mod registry;

pub use hash::PassthroughHasher;
pub use mem_odb::MemOdb;
pub use odb::{Bytes, Odb};
pub use registry::FlushGuard;
