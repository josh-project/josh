//! Workspace metadata parsed from git trees.
//!
//! Each workspace in the build graph stores its configuration as blobs in a git tree
//! (`label`, `output`, `cmd`, `image`/build-tree OID, sidecar specs, etc.). This
//! module reads those blobs and constructs typed [`WorkspaceMeta`] values for the
//! scheduler.

use std::path::Path;
use std::str::FromStr;

use crate::OutputMode;
use josh_compose_backend::NetworkPolicy;
use josh_core::cache;
use josh_core::filter::tree;
use josh_core::memodb;

/// Specification for a sidecar service that runs alongside a workspace step.
///
/// Sidecars provide auxiliary services (databases, caches, mock APIs) that the step's
/// command can reach over the network.
pub struct SidecarSpec {
    /// Logical name used for addressing and labeling.
    pub name: String,
    /// Build-tree OID of the image to run.
    pub image: gix_hash::ObjectId,
    /// Static environment variables set by the workspace config.
    pub env: Vec<(String, String)>,
    /// Environment variable names to forward from the host process (e.g. API keys, CI
    /// tokens). The host value must be non-empty.
    pub passthrough: Vec<(String, String)>,
    /// Template environment variables injected by the scheduler after the sidecar
    /// starts (e.g. `{SIDECAR_IP}` is replaced with the sidecar's address).
    pub inject: Vec<(String, String)>,
    pub port: u16,
}

pub struct WorkspaceMeta {
    /// Human-readable label (also used as a cache key component).
    pub label: String,
    /// Whether an output artifact is created and, if so, whether it is extracted.
    pub output: OutputMode,
    /// Command executed inside the environment. Defaults to `bash run.sh`.
    pub cmd: String,
    /// Persistent cache key shared across runs of this workspace.
    pub cache: Option<String>,
    pub network: NetworkPolicy,
    /// Tree OID of the image workspace. `None` for orchestrator-only workspaces.
    pub image: Option<gix_hash::ObjectId>,
    /// Tree OID of the workspace files mounted into the environment at `/worktree`.
    /// `None` for orchestrator-only workspaces.
    pub worktree: Option<gix_hash::ObjectId>,
    pub sidecars: Vec<SidecarSpec>,
}

/// Read a blob from a git tree at the given path. Returns None if not found.
pub fn read_blob(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
    path: &str,
) -> Option<String> {
    let entry = tree::get_path_entry(transaction, odb, tree_oid, Path::new(path)).ok()??;
    let content = tree::blob_bytes(odb, entry.oid.to_owned())?;
    Some(std::str::from_utf8(&content).ok()?.trim().to_string())
}

/// Read all entries from a subtree at `prefix`. Returns (name, oid) pairs.
pub fn read_tree_entries(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
    prefix: &str,
) -> Vec<(String, gix_hash::ObjectId)> {
    let Ok(Some(entry)) = tree::get_path_entry(transaction, odb, tree_oid, Path::new(prefix))
    else {
        return vec![];
    };
    let Ok(subtree) = tree::read_tree(transaction, odb, entry.oid.to_owned()) else {
        return vec![];
    };
    subtree
        .entries()
        .map(|e| {
            (
                String::from_utf8_lossy(e.filename).into_owned(),
                e.oid.to_owned(),
            )
        })
        .collect()
}

/// Read all blob entries from a subtree and return (name, content) pairs.
pub fn read_blob_entries(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
    prefix: &str,
) -> Vec<(String, String)> {
    read_tree_entries(transaction, odb, tree_oid, prefix)
        .into_iter()
        .filter_map(|(name, oid)| {
            let content = tree::blob_bytes(odb, oid)?;
            Some((name, std::str::from_utf8(&content).ok()?.trim().to_string()))
        })
        .collect()
}

/// Parse a workspace's configuration from its git tree.
///
/// Reads blobs named `label`, `output`, `cmd`, `cache`, `network`, and `image`, plus
/// the optional `worktree` subtree. Returns `None` for `image` and `worktree` when
/// the workspace is orchestrator-only (no image to build and no files to mount).
pub fn read_meta(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
) -> anyhow::Result<WorkspaceMeta> {
    let label = read_blob(transaction, odb, ws_tree, "label")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ws_tree.to_string());

    let output = match read_blob(transaction, odb, ws_tree, "output").as_deref() {
        Some("none") => OutputMode::None,
        Some("workdir") => OutputMode::Workdir,
        _ => OutputMode::Keep,
    };

    let cmd = read_blob(transaction, odb, ws_tree, "cmd")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "bash run.sh".to_string());

    let cache = read_blob(transaction, odb, ws_tree, "cache").filter(|s| !s.is_empty());

    let network = match read_blob(transaction, odb, ws_tree, "network").as_deref() {
        Some("host") => NetworkPolicy::Host,
        _ => NetworkPolicy::None,
    };

    let image = read_blob(transaction, odb, ws_tree, "image")
        .filter(|s| !s.is_empty())
        .map(|sha| {
            gix_hash::ObjectId::from_str(&sha)
                .map_err(|_| anyhow::anyhow!("invalid image SHA in workspace tree: {sha}"))
        })
        .transpose()?;

    let tree = tree::read_tree(transaction, odb, ws_tree)?;
    let worktree = tree.entry(b"worktree").map(|e| e.oid.to_owned());

    let sidecars = read_sidecars(transaction, odb, ws_tree)?;

    Ok(WorkspaceMeta {
        label,
        output,
        cmd,
        cache,
        network,
        image,
        worktree,
        sidecars,
    })
}

pub fn read_sidecars(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<SidecarSpec>> {
    let mut out = vec![];
    for (name, content) in read_blob_entries(transaction, odb, ws_tree, "sidecars") {
        let sidecar_tree = gix_hash::ObjectId::from_str(content.trim())
            .map_err(|_| anyhow::anyhow!("sidecar {name}: invalid tree SHA {content:?}"))?;
        let image_sha = read_blob(transaction, odb, sidecar_tree, "image")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("sidecar {name}: missing image"))?;
        let image = gix_hash::ObjectId::from_str(&image_sha)
            .map_err(|_| anyhow::anyhow!("sidecar {name}: invalid image SHA {image_sha:?}"))?;
        let port_str = read_blob(transaction, odb, sidecar_tree, "port")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("sidecar {name}: missing port"))?;
        let port: u16 = port_str
            .parse()
            .map_err(|_| anyhow::anyhow!("sidecar {name}: invalid port {port_str:?}"))?;
        out.push(SidecarSpec {
            name,
            image,
            env: read_blob_entries(transaction, odb, sidecar_tree, "env"),
            passthrough: read_blob_entries(transaction, odb, sidecar_tree, "passthrough"),
            inject: read_blob_entries(transaction, odb, sidecar_tree, "inject"),
            port,
        });
    }
    Ok(out)
}
