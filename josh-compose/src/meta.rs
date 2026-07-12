//! Workspace metadata parsed from git trees.
//!
//! Each workspace in the build graph stores its configuration as blobs in a git tree
//! (`label`, `output`, `cmd`, `image`/build-tree OID, sidecar specs, etc.). This
//! module reads those blobs and constructs typed [`WorkspaceMeta`] values for the
//! scheduler.

use std::path::Path;

pub use crate::OutputMode;
pub use josh_compose_runtime::NetworkPolicy;

/// Specification for a sidecar service that runs alongside a workspace step.
///
/// Sidecars provide auxiliary services (databases, caches, mock APIs) that the step's
/// command can reach over the network.
pub struct SidecarSpec {
    /// Logical name used for addressing and labeling.
    pub name: String,
    /// Build-tree OID of the image to run.
    pub image: git2::Oid,
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
    pub image: Option<git2::Oid>,
    /// Tree OID of the workspace files mounted into the environment at `/worktree`.
    /// `None` for orchestrator-only workspaces.
    pub worktree: Option<git2::Oid>,
    pub sidecars: Vec<SidecarSpec>,
}

/// Read a blob from a git tree at the given path. Returns None if not found.
pub fn read_blob(repo: &git2::Repository, tree_oid: git2::Oid, path: &str) -> Option<String> {
    let tree = repo.find_tree(tree_oid).ok()?;
    let entry = tree.get_path(Path::new(path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    let content = std::str::from_utf8(blob.content()).ok()?.trim().to_string();
    Some(content)
}

/// Read all entries from a subtree at `prefix`. Returns (name, oid) pairs.
pub fn read_tree_entries(
    repo: &git2::Repository,
    tree_oid: git2::Oid,
    prefix: &str,
) -> Vec<(String, git2::Oid)> {
    let Ok(tree) = repo.find_tree(tree_oid) else {
        return vec![];
    };
    let Ok(entry) = tree.get_path(Path::new(prefix)) else {
        return vec![];
    };
    let Ok(subtree) = repo.find_tree(entry.id()) else {
        return vec![];
    };
    subtree
        .iter()
        .map(|e| (e.name().unwrap_or("").to_string(), e.id()))
        .collect()
}

/// Read all blob entries from a subtree and return (name, content) pairs.
pub fn read_blob_entries(
    repo: &git2::Repository,
    tree_oid: git2::Oid,
    prefix: &str,
) -> Vec<(String, String)> {
    read_tree_entries(repo, tree_oid, prefix)
        .into_iter()
        .filter_map(|(name, oid)| {
            let blob = repo.find_blob(oid).ok()?;
            let content = std::str::from_utf8(blob.content()).ok()?.trim().to_string();
            Some((name, content))
        })
        .collect()
}

/// Parse a workspace's configuration from its git tree.
///
/// Reads blobs named `label`, `output`, `cmd`, `cache`, `network`, `image`, and
/// optionally `worktree` (falling back to `run` for backward compatibility) from the
/// tree. Returns `None` for `image` and `worktree` when the workspace is
/// orchestrator-only (no image to build and no files to mount).
pub fn read_meta(repo: &git2::Repository, ws_tree: git2::Oid) -> anyhow::Result<WorkspaceMeta> {
    let label = read_blob(repo, ws_tree, "label")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ws_tree.to_string());

    let output = match read_blob(repo, ws_tree, "output").as_deref() {
        Some("none") => OutputMode::None,
        Some("workdir") => OutputMode::Workdir,
        _ => OutputMode::Keep,
    };

    let cmd = read_blob(repo, ws_tree, "cmd")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "bash run.sh".to_string());

    let cache = read_blob(repo, ws_tree, "cache").filter(|s| !s.is_empty());

    let network = match read_blob(repo, ws_tree, "network").as_deref() {
        Some("host") => NetworkPolicy::Host,
        _ => NetworkPolicy::None,
    };

    let image = read_blob(repo, ws_tree, "image")
        .filter(|s| !s.is_empty())
        .map(|sha| {
            git2::Oid::from_str(&sha)
                .map_err(|_| anyhow::anyhow!("invalid image SHA in workspace tree: {sha}"))
        })
        .transpose()?;

    let tree = repo.find_tree(ws_tree)?;
    // Prefer "worktree"; fall back to "run" for backward compatibility with
    // workspaces authored before the rename.
    let worktree = tree
        .get_path(Path::new("worktree"))
        .map(|e| e.id())
        .or_else(|_| tree.get_path(Path::new("run")).map(|e| e.id()))
        .ok();

    let sidecars = read_sidecars(repo, ws_tree)?;

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
    repo: &git2::Repository,
    ws_tree: git2::Oid,
) -> anyhow::Result<Vec<SidecarSpec>> {
    let mut out = vec![];
    for (name, content) in read_blob_entries(repo, ws_tree, "sidecars") {
        let sidecar_tree = git2::Oid::from_str(content.trim())
            .map_err(|_| anyhow::anyhow!("sidecar {name}: invalid tree SHA {content:?}"))?;
        let image_sha = read_blob(repo, sidecar_tree, "image")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("sidecar {name}: missing image"))?;
        let image = git2::Oid::from_str(&image_sha)
            .map_err(|_| anyhow::anyhow!("sidecar {name}: invalid image SHA {image_sha:?}"))?;
        let port_str = read_blob(repo, sidecar_tree, "port")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("sidecar {name}: missing port"))?;
        let port: u16 = port_str
            .parse()
            .map_err(|_| anyhow::anyhow!("sidecar {name}: invalid port {port_str:?}"))?;
        out.push(SidecarSpec {
            name,
            image,
            env: read_blob_entries(repo, sidecar_tree, "env"),
            passthrough: read_blob_entries(repo, sidecar_tree, "passthrough"),
            inject: read_blob_entries(repo, sidecar_tree, "inject"),
            port,
        });
    }
    Ok(out)
}
