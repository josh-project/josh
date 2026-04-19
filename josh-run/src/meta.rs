use std::path::Path;

pub use crate::OutputMode;

#[derive(Debug, Clone, PartialEq)]
pub enum NetworkMode {
    None,
    Host,
}

pub struct WorkspaceMeta {
    pub label: String,
    pub output: OutputMode,
    pub cmd: String,
    pub cache: Option<String>,
    pub network: NetworkMode,
    /// Tree OID of the image workspace. None for orchestrator-only workspaces.
    pub image: Option<git2::Oid>,
    /// Tree OID of the files to place in the container. None for orchestrator-only workspaces.
    pub worktree: Option<git2::Oid>,
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
        Some("host") => NetworkMode::Host,
        _ => NetworkMode::None,
    };

    let image = read_blob(repo, ws_tree, "image")
        .filter(|s| !s.is_empty())
        .map(|sha| {
            git2::Oid::from_str(&sha)
                .map_err(|_| anyhow::anyhow!("invalid image SHA in workspace tree: {sha}"))
        })
        .transpose()?;

    let tree = repo.find_tree(ws_tree)?;
    let worktree = tree
        .get_path(Path::new("worktree"))
        .map(|e| e.id())
        .or_else(|_| tree.get_path(Path::new("run")).map(|e| e.id()))
        .ok();

    Ok(WorkspaceMeta {
        label,
        output,
        cmd,
        cache,
        network,
        image,
        worktree,
    })
}
