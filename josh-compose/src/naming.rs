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
pub fn output(ws_tree: gix_hash::ObjectId) -> String {
    format!("{OUTPUT_PREFIX}{ws_tree}")
}

/// Persistent cache artifact named `cache_name` (mounted at `/opt/cache`).
pub fn cache(cache_name: &str) -> String {
    format!("{CACHE_PREFIX}{cache_name}")
}

/// Environment key for the image built from `build_tree`.
pub fn env(build_tree: gix_hash::ObjectId) -> String {
    format!("{ENV_PREFIX}{build_tree}")
}

pub(crate) fn output_oid(name: &str) -> Option<gix_hash::ObjectId> {
    parse_oid(name.strip_prefix(OUTPUT_PREFIX)?)
}

pub(crate) fn env_oid(name: &str) -> Option<gix_hash::ObjectId> {
    let (_, suffix) = name.split_once(ENV_PREFIX)?;
    parse_oid(suffix)
}

fn parse_oid(value: &str) -> Option<gix_hash::ObjectId> {
    let hex = value.get(..40)?;
    if value.as_bytes().get(40).is_some_and(u8::is_ascii_hexdigit) {
        return None;
    }
    gix_hash::ObjectId::from_hex(hex.as_bytes()).ok()
}

pub const OUTPUT_PREFIX: &str = "josh_out_";
pub const ENV_PREFIX: &str = "josh_ws_image_";
pub const CACHE_PREFIX: &str = "josh_cache_";

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn parses_runtime_resource_names() {
        assert_eq!(
            output_oid(&format!("{OUTPUT_PREFIX}{OID}"))
                .unwrap()
                .to_string(),
            OID
        );
        assert_eq!(
            env_oid(&format!("localhost/{ENV_PREFIX}{OID}:latest"))
                .unwrap()
                .to_string(),
            OID
        );
        assert!(output_oid("not-a-compose-volume").is_none());
        assert!(env_oid(&format!("{ENV_PREFIX}{OID}0")).is_none());
    }
}
