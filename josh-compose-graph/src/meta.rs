//! Decodes compose workspace trees into scheduler metadata.
//! Scalar settings are blobs; object references are gitlinks.

use std::path::Path;

use crate::{NetworkPolicy, OutputMode};
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

pub fn read_gitlink(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
    path: &str,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let Some(entry) = tree::get_path_entry(transaction, odb, tree_oid, Path::new(path))? else {
        return Ok(None);
    };
    if !entry.mode.is_commit() {
        anyhow::bail!("expected gitlink at {path}");
    }
    Ok(Some(entry.oid))
}

pub fn read_gitlink_entries(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    tree_oid: gix_hash::ObjectId,
    prefix: &str,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let Ok(Some(entry)) = tree::get_path_entry(transaction, odb, tree_oid, Path::new(prefix))
    else {
        return Ok(vec![]);
    };
    let Ok(subtree) = tree::read_tree(transaction, odb, entry.oid) else {
        return Ok(vec![]);
    };
    Ok(subtree
        .entries()
        .filter(|entry| entry.mode.is_commit())
        .map(|entry| {
            (
                String::from_utf8_lossy(entry.filename).into_owned(),
                entry.oid.to_owned(),
            )
        })
        .collect())
}

/// Decode a workspace tree.
/// Missing image and worktree entries represent orchestrator-only nodes.
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

    let image = read_gitlink(transaction, odb, ws_tree, "image")?;

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
    for (name, sidecar_tree) in read_gitlink_entries(transaction, odb, ws_tree, "sidecars")? {
        let image = read_gitlink(transaction, odb, sidecar_tree, "image")?
            .ok_or_else(|| anyhow::anyhow!("sidecar {name}: missing image"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_references_are_gitlinks() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let odb = transaction.odb();
        let leaf = gix_hash::ObjectId::from_bytes_or_panic(&[1; 20]);
        let target =
            tree::insert_oid(odb, tree::empty_id(), Path::new("leaf"), leaf, 0o100644).unwrap();

        let inputs = tree::insert_oid(
            odb,
            tree::empty_id(),
            Path::new("dependency"),
            target,
            0o160000,
        )
        .unwrap();
        let workspace =
            tree::insert_oid(odb, tree::empty_id(), Path::new("image"), target, 0o160000).unwrap();
        let workspace =
            tree::insert_oid(odb, workspace, Path::new("inputs"), inputs, 0o040000).unwrap();
        let workspace =
            tree::insert_oid(odb, workspace, Path::new("invalid"), target, 0o100644).unwrap();

        assert_eq!(
            read_meta(&transaction, odb, workspace).unwrap().image,
            Some(target)
        );
        assert_eq!(
            read_gitlink_entries(&transaction, odb, workspace, "inputs").unwrap(),
            vec![("dependency".to_string(), target)]
        );
        assert_eq!(
            read_gitlink(&transaction, odb, workspace, "invalid")
                .unwrap_err()
                .to_string(),
            "expected gitlink at invalid"
        );
    }
}
