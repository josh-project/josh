pub mod commands;
pub mod config;
pub mod forge;
pub mod remote_ops;

/// Default cap (128 MiB) on a transaction's in-memory object buffer; exceeding it flushes a
/// packfile.
pub const MAX_MEM_PACK_SIZE: usize = 128 * 1024 * 1024;
