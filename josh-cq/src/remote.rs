use anyhow::Context;
use anyhow::anyhow;

use josh_core::git::normalize_repo_path;

use std::collections::BTreeMap;
use std::ops::Deref;
use std::process::Command;

/// Resolve the HEAD symref on a remote repository
///
/// Returns the ref that HEAD points to (e.g. "refs/heads/master")
pub fn resolve_head_symref(url: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["ls-remote", "--symref", url, "HEAD"])
        .output()
        .context("Failed to execute git ls-remote --symref")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("git ls-remote --symref failed: {}", stderr));
    }

    let stdout = String::from_utf8(output.stdout)?;
    for line in stdout.lines() {
        // Format: ref: refs/heads/master\tHEAD
        if let Some(rest) = line.strip_prefix("ref: ") {
            if let Some(target) = rest.split('\t').next() {
                return Ok(target.to_string());
            }
        }
    }

    Err(anyhow::anyhow!(
        "HEAD is not a symref or not found at: {}",
        url
    ))
}

/// List refs from a remote repository using git ls-remote
///
/// Returns a map of ref names to their OIDs
pub fn list_refs(url: &str) -> anyhow::Result<BTreeMap<String, git2::Oid>> {
    let output = Command::new("git")
        .args(["ls-remote", url])
        .output()
        .context("Failed to execute git ls-remote")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("git ls-remote failed: {}", stderr));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let refs: BTreeMap<String, git2::Oid> = stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() == 2 {
                let oid = git2::Oid::from_str(parts[0]).ok()?;
                Some((parts[1].to_string(), oid))
            } else {
                None
            }
        })
        .collect();

    if refs.is_empty() {
        return Err(anyhow!("No refs found at remote URL: {}", url));
    }

    Ok(refs)
}

// TODO: this will eventually be replaced with a fetch that
// TODO: doesn't update local refs after receive-pack
struct TempNamespace {
    repo_path: std::path::PathBuf,
    name: String,
}

impl TempNamespace {
    pub fn new(repo_path: impl AsRef<std::path::Path>) -> Self {
        Self {
            repo_path: repo_path.as_ref().into(),
            name: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl Deref for TempNamespace {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.name.as_str()
    }
}

impl Drop for TempNamespace {
    fn drop(&mut self) {
        let ns_path = self.repo_path.join("refs/namespaces").join(&self.name);
        let _ = std::fs::remove_dir_all(&ns_path);
    }
}

/// Fetch without updating the refs -- refs go into a temporary
/// namespace that is deleted when function exits
pub fn fetch(repo: &git2::Repository, url: &str) -> anyhow::Result<BTreeMap<String, git2::Oid>> {
    let ns = TempNamespace::new(repo.path());
    let path = normalize_repo_path(repo.path());

    let refspec = format!("+refs/*:refs/namespaces/{}/refs/*", ns.deref());
    let ns_prefix = format!("refs/namespaces/{}/", ns.deref());

    let output = Command::new("git")
        .current_dir(&path)
        .args(["fetch", "--porcelain", "-v", url, &refspec])
        .output()
        .context("Failed to execute git fetch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("git fetch failed: {}", stderr));
    }

    // Parse porcelain output: <flag> <old-oid> <new-oid> <ref-name>
    let stdout = String::from_utf8(output.stdout)?;
    let refs: BTreeMap<String, git2::Oid> = stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Format: <flag> <old-oid> <new-oid> <local-ref>
            if parts.len() >= 4 {
                let new_oid = git2::Oid::from_str(parts[2]).ok()?;
                // Strip namespace prefix from ref name
                let ref_name = parts[3].strip_prefix(&ns_prefix).unwrap_or(parts[3]);
                Some((ref_name.to_string(), new_oid))
            } else {
                None
            }
        })
        .collect();

    Ok(refs)
}
