//! Single source of truth for the metarepo layout.
//!
//! Every tracked remote `<name>` lives under:
//!
//! ```text
//! remotes/<name>/meta/remote.json      # RemoteMeta (settings)
//! remotes/<name>/contents/...          # remote content + history (prefix-filtered)
//! remotes/<name>/workspace/workspace.josh   # the stored workspace filter
//! ```
//!
//! All paths use forward slashes (git tree paths); the helpers here are the only
//! place these strings are constructed so the layout is not scattered across the
//! codebase.

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Top-level directory under which every tracked remote lives.
pub const REMOTES_DIR: &str = "remotes";
/// Name of the stored workspace filter file inside each remote's workspace dir.
pub const WORKSPACE_JOSH: &str = "workspace.josh";

/// Settings for a tracked remote, stored at [`remote_meta_path`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteMeta {
    /// Clone URL of the tracked remote.
    pub url: String,
    /// Ref tracked on the remote (e.g. `"HEAD"`).
    #[serde(rename = "ref")]
    pub ref_: String,
}

/// `remotes/<name>` — the root of a single tracked remote.
pub fn remote_dir(name: &str) -> String {
    format!("{REMOTES_DIR}/{name}")
}

/// `remotes/<name>/contents` — where the remote's content/history lives.
pub fn contents_dir(name: &str) -> String {
    format!("{}/contents", remote_dir(name))
}

/// `remotes/<name>/workspace` — the workspace root for the remote.
pub fn workspace_dir(name: &str) -> String {
    format!("{}/workspace", remote_dir(name))
}

/// `remotes/<name>/workspace/workspace.josh` — the stored workspace filter file.
pub fn workspace_josh_path(name: &str) -> String {
    format!("{}/{WORKSPACE_JOSH}", workspace_dir(name))
}

/// `remotes/<name>/meta/remote.json` — the remote's settings file.
pub fn remote_meta_path(name: &str) -> String {
    format!("{}/meta/remote.json", remote_dir(name))
}

/// Filter spec that moves freshly-fetched remote content under
/// `remotes/<name>/contents`, preserving history.
pub fn contents_prefix_filter_spec(name: &str) -> String {
    format!(":prefix={}", contents_dir(name))
}

/// Body of the `workspace.josh` stored for a remote: surface its `contents/` at
/// the workspace root.
pub fn workspace_josh_content(name: &str) -> String {
    format!(":/{}\n", contents_dir(name))
}

/// The filter that reconstructs a tracked remote's main branch from the metarepo.
///
/// `:workspace=…` surfaces the remote's `contents/` at root (via the stored
/// `workspace.josh`) plus the workspace root's own `workspace.josh`;
/// `:exclude[::workspace.josh]` strips that stray file, leaving exactly the
/// remote's tree and history.
pub fn workspace_filter_spec(name: &str) -> String {
    format!(
        ":workspace={}:exclude[::{WORKSPACE_JOSH}]",
        workspace_dir(name)
    )
}

/// Discover all tracked remotes by reading `remotes/*/meta/remote.json` from a
/// metarepo tree. Returns `(name, meta)` pairs. Entries that lack a readable
/// `remote.json` are skipped.
pub fn list_tracked_remotes(
    repo: &git2::Repository,
    tree: &git2::Tree,
) -> anyhow::Result<Vec<(String, RemoteMeta)>> {
    let remotes_entry = match tree.get_path(Path::new(REMOTES_DIR)) {
        Ok(entry) => entry,
        Err(_) => return Ok(Vec::new()),
    };
    let remotes_tree = repo
        .find_tree(remotes_entry.id())
        .context("Failed to find remotes tree")?;

    let mut out = Vec::new();
    for entry in remotes_tree.iter() {
        let Some(name) = entry.name() else { continue };
        if entry.kind() != Some(git2::ObjectType::Tree) {
            continue;
        }
        let Ok(meta_entry) = tree.get_path(Path::new(&remote_meta_path(name))) else {
            continue;
        };
        let Ok(blob) = repo.find_blob(meta_entry.id()) else {
            continue;
        };
        match serde_json::from_slice::<RemoteMeta>(blob.content()) {
            Ok(meta) => out.push((name.to_string(), meta)),
            Err(e) => tracing::warn!(remote = %name, error = ?e, "invalid remote.json; skipping"),
        }
    }
    Ok(out)
}
