use anyhow::Context;
use anyhow::anyhow;

use std::collections::BTreeMap;
use std::process::Command;

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
