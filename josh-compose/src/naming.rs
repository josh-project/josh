//! Artifact and environment naming conventions.
//!
//! The runtime addresses artifacts and environments by opaque string keys. These
//! keys are necessarily produced here — the scheduler owns the translation of git
//! OIDs / workspace names into keys (the runtime is git-agnostic). Centralizing
//! the scheme in one module keeps creation, cache checks, planning, and cleanup
//! in sync. The runtime itself treats every name as opaque.
//!
//! Every key carries a `josh_` marker so resources created by the runtime are
//! unambiguous and don't collide with anything else on the system.

/// Output artifact for the workspace tree `ws_tree` (mounted at `/out`).
pub fn output(ws_tree: git2::Oid) -> String {
    format!("{OUTPUT_PREFIX}{ws_tree}")
}

/// Persistent cache artifact named `cache_name` (mounted at `/opt/cache`).
pub fn cache(cache_name: &str) -> String {
    format!("{CACHE_PREFIX}{cache_name}")
}

/// Environment key for the image built from `build_tree`.
pub fn env(build_tree: git2::Oid) -> String {
    format!("{ENV_PREFIX}{build_tree}")
}

pub const OUTPUT_PREFIX: &str = "josh_out_";
pub const ENV_PREFIX: &str = "josh_ws_image_";
pub const CACHE_PREFIX: &str = "josh_cache_";
