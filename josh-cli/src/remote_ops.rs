use anyhow::Context;

use josh_core::git::{normalize_repo_path, spawn_git_command};

/// Parse the symref output from `git ls-remote --symref` to extract the default branch.
/// Returns `(branch_name, full_ref)` e.g. `("master", "refs/remotes/origin/master")`.
pub fn try_parse_symref(remote: &str, output: &str) -> Option<(String, String)> {
    let line = output.lines().next()?;
    let symref_part = line.split('\t').next()?;

    let default_branch = symref_part.strip_prefix("ref: refs/heads/")?;
    let default_branch_ref = format!("refs/remotes/{}/{}", remote, default_branch);

    Some((default_branch.to_string(), default_branch_ref))
}

/// Query the remote's HEAD branch via `git ls-remote --symref`.
/// Falls back to `"master"` if the remote does not advertise a symref.
pub fn get_head_branch(
    url: &str,
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(["ls-remote", "--symref", url, "HEAD"])
        .current_dir(repo_path)
        .output()
        .context("Failed to run git ls-remote")?;

    if output.status.success() {
        let text = String::from_utf8(output.stdout).context("Invalid ls-remote output")?;
        if let Some((branch, _)) = try_parse_symref(remote_name, &text) {
            return Ok(branch);
        }
    }

    Ok("master".to_string())
}

/// Apply a josh filter to all refs under `refs/josh/remotes/{remote_name}/*`.
/// Returns a list of `(branch_name, filtered_oid)` pairs (zero OIDs omitted).
pub fn filter_remote_refs(
    transaction: &josh_core::cache::Transaction,
    filter: josh_core::filter::Filter,
    remote_name: &str,
) -> anyhow::Result<Vec<(String, git2::Oid)>> {
    let input_refs = get_backing_refs(transaction, remote_name)?;
    let prefix = format!("refs/josh/remotes/{}/", remote_name);

    let (updated_refs, errors) = josh_core::filter_refs(transaction, filter, &input_refs);

    if let Some(error) = errors.into_iter().next() {
        return Err(anyhow::anyhow!("josh filter error: {}", error.1));
    }

    let result = updated_refs
        .into_iter()
        .filter(|(_, oid)| *oid != git2::Oid::zero())
        .map(|(original_ref, oid)| {
            let branch = original_ref
                .strip_prefix(&prefix)
                .unwrap_or(&original_ref)
                .to_string();
            (branch, oid)
        })
        .collect();

    Ok(result)
}

/// Return raw `(refname, oid)` pairs for all refs under
/// `refs/josh/remotes/{remote_name}/*`, suitable as input to
/// `josh_core::filter_refs`.  Errors if no refs are found.
pub fn get_backing_refs(
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
) -> anyhow::Result<Vec<(String, git2::Oid)>> {
    let repo = transaction.repo();
    let mut input_refs = Vec::new();
    let josh_remotes = repo.references_glob(&format!("refs/josh/remotes/{}/*", remote_name))?;

    for reference in josh_remotes {
        let reference = reference?;
        if let Some(target) = reference.target() {
            let ref_name = reference.name().unwrap().to_string();
            input_refs.push((ref_name, target));
        }
    }

    if input_refs.is_empty() {
        return Err(anyhow::anyhow!(
            "No remote references found for '{}'",
            remote_name
        ));
    }

    Ok(input_refs)
}

/// Apply a josh filter to all refs under `refs/josh/remotes/{remote_name}/*` and write
/// the filtered commits to `refs/namespaces/josh-{remote_name}/refs/heads/*`.
/// Then runs `git fetch {remote_name}` to expose them through the configured remote.
pub fn apply_josh_filtering(
    transaction: &josh_core::cache::Transaction,
    repo_path: &std::path::Path,
    filter: josh_core::filter::Filter,
    remote_name: &str,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    for (branch_name, filtered_oid) in filter_remote_refs(transaction, filter, remote_name)? {
        let filtered_ref = format!(
            "refs/namespaces/josh-{}/refs/heads/{}",
            remote_name, branch_name
        );
        repo.reference(&filtered_ref, filtered_oid, true, "josh filter")
            .context("failed to create filtered reference")?;
    }

    spawn_git_command(
        normalize_repo_path(repo_path).as_path(),
        &["fetch", remote_name],
        &[],
    )
    .context("failed to fetch filtered refs")?;

    Ok(())
}
