use anyhow::Context;

use josh_core::git::normalize_repo_path;

use crate::config::{RemoteConfig, read_remote_config};
use crate::porcelain::RefUpdate;
use crate::remote_ops;

#[derive(Debug, clap::Parser)]
pub struct FetchArgs {
    /// Remote name (or URL) to fetch from
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// Ref to fetch (branch, tag, or commit-ish)
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,
}

/// Fetch unfiltered refs, apply projection filtering, and fetch the filtered
/// refs through the namespace remote. Returns the ref updates reported by the
/// final (namespaced) fetch; does not integrate anything into local branches.
pub fn handle_fetch(
    args: &FetchArgs,
    transaction: &josh_core::cache::Transaction,
    distributed_cache: bool,
) -> anyhow::Result<Vec<RefUpdate>> {
    let repo_path = normalize_repo_path(transaction.path());

    let config = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;
    let filter = config.semantic_filter();
    let RemoteConfig { url, ref_spec, .. } = config;

    // This backing fetch is the one that actually transfers objects from the
    // remote: --porcelain sends the internal ref-update listing to stdout
    // (captured and discarded) while transfer progress stays on stderr,
    // which is forwarded to the user.
    transaction
        .git_command(&["fetch", "--porcelain", &url, &ref_spec], &[])?
        .with_stdout(std::process::Stdio::piped())
        .spawn()
        .context("git fetch to josh/remotes failed")?;

    if distributed_cache {
        if let Err(e) = crate::commands::cache::fetch_remote_cache(transaction, &url, filter) {
            eprintln!("Warning: could not fetch remote cache: {e}");
        }
    }

    // Resolve the default branch from the remote's HEAD symref.
    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);

    // ls-remote --symref output format: "ref: refs/heads/main\t<commit-hash>"
    let output = std::process::Command::new("git")
        .args(["ls-remote", "--symref", &url, "HEAD"])
        .current_dir(&repo_path)
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to determine default branch: git ls-remote --symref failed for '{}'",
            args.remote
        ));
    }

    let ls_output = String::from_utf8(output.stdout)?;
    let (default_branch, default_branch_ref) =
        remote_ops::try_parse_symref(&args.remote, &ls_output).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not determine default branch from remote '{}': \
                 no symref for HEAD in ls-remote output",
                args.remote
            )
        })?;

    transaction.create_symref(&head_ref, &default_branch_ref, "josh remote HEAD")?;
    transaction.create_symref(
        &format!("refs/namespaces/josh-{}/{}", args.remote, "HEAD"),
        &format!("refs/heads/{}", default_branch),
        "josh remote HEAD",
    )?;

    // Updates refs only; checkout is left to the caller.
    remote_ops::apply_josh_filtering(transaction, filter, &args.remote, &default_branch)
}
