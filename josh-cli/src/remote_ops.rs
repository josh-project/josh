use anyhow::Context;

use josh_core::filter::{self, Filter, flatten_chain};
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

/// Resolve the default branch name from the locally stored
/// `refs/remotes/{remote_name}/HEAD` symref.
///
/// Returns an error if the symref cannot be resolved (e.g. the remote
/// has not been fetched yet or does not advertise HEAD).
pub fn resolve_default_branch(
    repo: &git2::Repository,
    remote_name: &str,
) -> anyhow::Result<String> {
    let head_symref = format!("refs/remotes/{}/HEAD", remote_name);
    repo.find_reference(&head_symref)
        .ok()
        .and_then(|r| r.symbolic_target().map(|s| s.to_string()))
        .and_then(|target| {
            target
                .strip_prefix(&format!("refs/remotes/{}/", remote_name))
                .map(|s| s.to_string())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not resolve default branch from '{}'. \
                 Has the remote been fetched?",
                head_symref
            )
        })
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

/// Build the ref-path prefix for step `step_idx` in a chain.
///
/// The path encodes the filter history newest-first so that each ref path
/// uniquely identifies both *what* was applied and *to what* it was applied:
///
/// - step 0 of [A, B, C] → `"{A_id}"`
/// - step 1 of [A, B, C] → `"{B_id}/{A_id}"`
/// - step 2 of [A, B, C] → `"{C_id}/{B_id}/{A_id}"`
pub fn step_ref_prefix(step_idx: usize, steps: &[Filter]) -> String {
    (0..=step_idx)
        .rev()
        .map(|i| steps[i].id().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Apply a josh filter to all refs under `refs/josh/remotes/{remote_name}/*` and write
/// the filtered commits to `refs/namespaces/josh-{remote_name}/refs/heads/*`.
/// Also writes `refs/josh/filtered/` refs for the default branch and persists filter tree objects.
/// Then runs `git fetch {remote_name}` to expose them through the configured remote.
pub fn apply_josh_filtering(
    transaction: &josh_core::cache::Transaction,
    repo_path: &std::path::Path,
    filter: josh_core::filter::Filter,
    remote_name: &str,
    default_branch: &str,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let prefix = format!("refs/josh/remotes/{}/", remote_name);

    let steps = flatten_chain(filter);

    // Seed with the raw backing refs
    let mut current_commits: Vec<(String, git2::Oid)> = get_backing_refs(transaction, remote_name)?
        .into_iter()
        .map(|(refname, oid)| {
            let branch = refname
                .strip_prefix(&prefix)
                .unwrap_or(&refname)
                .to_string();
            (branch, oid)
        })
        .collect();

    // Apply each step, writing filtered refs along the way
    for (step_idx, step_filter) in steps.iter().enumerate() {
        let (filtered, errors) =
            josh_core::filter_refs(transaction, *step_filter, &current_commits);

        if let Some(error) = errors.into_iter().next() {
            return Err(anyhow::anyhow!("josh filter error: {}", error.1));
        }

        // Persist the filter tree object to ODB so cache build can reconstruct it
        filter::as_tree(repo, *step_filter)?;

        let prefix_path = step_ref_prefix(step_idx, &steps);
        let mut next_commits = Vec::new();

        for (branch_name, filtered_oid) in &filtered {
            if *filtered_oid == git2::Oid::zero() {
                continue;
            }

            // Write refs/josh/filtered/ ref only for the default branch
            if branch_name == default_branch {
                let filtered_ref =
                    format!("refs/josh/filtered/{}/heads/{}", prefix_path, branch_name);
                repo.reference(&filtered_ref, *filtered_oid, true, "josh filter")
                    .with_context(|| format!("failed to write filtered ref '{}'", filtered_ref))?;
            }

            next_commits.push((branch_name.clone(), *filtered_oid));
        }

        current_commits = next_commits;
    }

    // Write namespace refs from the final step results (existing behavior)
    for (branch_name, filtered_oid) in &current_commits {
        let ns_ref = format!(
            "refs/namespaces/josh-{}/refs/heads/{}",
            remote_name, branch_name
        );
        repo.reference(&ns_ref, *filtered_oid, true, "josh filter")
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
