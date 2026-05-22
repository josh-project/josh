use anyhow::Context;

use josh_core::cache::{
    CACHE_VERSION, CacheStack, DistributedCacheBackend, Transaction, TransactionContext,
};
use josh_core::filter::{Filter, flatten_chain};
use josh_core::git::{normalize_repo_path, spawn_git_command};

use crate::config::{RemoteConfig, read_remote_config};
use crate::remote_ops;

#[derive(Debug, clap::Parser)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum CacheCommand {
    /// Build the distributed cache by applying the configured filter to already-fetched refs
    Build(CacheBuildArgs),
    /// Push the distributed cache and filtered refs to the backing remote
    Push(CachePushArgs),
    /// Fetch the distributed cache and filtered refs from the remote
    Fetch(CacheFetchArgs),
}

#[derive(Debug, clap::Parser)]
pub struct CacheBuildArgs {
    /// Remote name (defaults to "origin")
    #[arg(default_value = "origin")]
    pub remote: String,
}

#[derive(Debug, clap::Parser)]
pub struct CachePushArgs {
    /// Remote name (defaults to "origin")
    #[arg(default_value = "origin")]
    pub remote: String,
}

#[derive(Debug, clap::Parser)]
pub struct CacheFetchArgs {
    /// Remote name (defaults to "origin")
    #[arg(default_value = "origin")]
    pub remote: String,
}

pub fn handle_cache(args: &CacheArgs, transaction: &Transaction) -> anyhow::Result<()> {
    match &args.command {
        CacheCommand::Build(a) => handle_cache_build(a, transaction),
        CacheCommand::Push(a) => handle_cache_push(a, transaction),
        CacheCommand::Fetch(a) => handle_cache_fetch(a, transaction),
    }
}

/// Build the ref-path prefix for step `step_idx` in a chain.
///
/// The path encodes the filter history newest-first so that each ref path
/// uniquely identifies both *what* was applied and *to what* it was applied:
///
/// - step 0 of [A, B, C] → `"{A_id}"`
/// - step 1 of [A, B, C] → `"{B_id}/{A_id}"`
/// - step 2 of [A, B, C] → `"{C_id}/{B_id}/{A_id}"`
fn step_ref_prefix(step_idx: usize, steps: &[Filter]) -> String {
    (0..=step_idx)
        .rev()
        .map(|i| steps[i].id().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn handle_cache_build(args: &CacheBuildArgs, transaction: &Transaction) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let RemoteConfig {
        filter_with_meta, ..
    } = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    let filter = filter_with_meta.peel();
    let steps = flatten_chain(filter);

    // Build a transaction that has ONLY the DistributedCacheBackend — no Sled.
    // This ensures every filter operation writes a new entry to the distributed
    // cache even if Sled already has a result for that commit.
    let cache = std::sync::Arc::new(CacheStack::new().with_backend(
        DistributedCacheBackend::new(&repo_path).context("Failed to open distributed cache")?,
    ));
    let build_transaction = TransactionContext::new(&repo_path, cache)
        .open(None)
        .context("Failed to open build transaction")?;

    // Seed with the raw backing refs; the ref names act as branch labels.
    let prefix = format!("refs/josh/remotes/{}/", args.remote);
    let mut current_commits: Vec<(String, git2::Oid)> =
        remote_ops::get_backing_refs(&build_transaction, &args.remote)?
            .into_iter()
            .map(|(refname, oid)| {
                let branch = refname
                    .strip_prefix(&prefix)
                    .unwrap_or(&refname)
                    .to_string();
                (branch, oid)
            })
            .collect();

    // Walk through each step in the chain, applying it to the previous step's
    // results and writing an intermediate filtered ref.
    for (step_idx, step_filter) in steps.iter().enumerate() {
        let (filtered, errors) =
            josh_core::filter_refs(&build_transaction, *step_filter, &current_commits);

        if let Some(error) = errors.into_iter().next() {
            return Err(anyhow::anyhow!("filter error: {}", error.1));
        }

        let prefix_path = step_ref_prefix(step_idx, &steps);
        let mut next_commits = Vec::new();

        for (branch_name, filtered_oid) in filtered {
            if filtered_oid == git2::Oid::zero() {
                continue;
            }
            let filtered_ref = format!("refs/josh/filtered/{}/heads/{}", prefix_path, branch_name);
            repo.reference(&filtered_ref, filtered_oid, true, "josh cache build")
                .with_context(|| format!("failed to write filtered ref '{}'", filtered_ref))?;
            next_commits.push((branch_name, filtered_oid));
        }

        current_commits = next_commits;
    }

    eprintln!("Built cache for remote '{}'", args.remote);
    Ok(())
}

fn handle_cache_push(args: &CachePushArgs, transaction: &Transaction) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let RemoteConfig {
        url,
        filter_with_meta,
        ..
    } = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    let filter = filter_with_meta.peel();
    let steps = flatten_chain(filter);

    // Resolve the default branch from the locally stored HEAD symref.
    // This is set by `josh fetch`; fall back to "master" if not present.
    let head_symref = format!("refs/remotes/{}/HEAD", args.remote);
    let default_branch = repo
        .find_reference(&head_symref)
        .ok()
        .and_then(|r| r.symbolic_target().map(|s| s.to_string()))
        .and_then(|target| {
            target
                .strip_prefix(&format!("refs/remotes/{}/", args.remote))
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            eprintln!(
                "Warning: could not resolve default branch from '{}', falling back to 'master'",
                head_symref
            );
            "master".to_string()
        });

    // Push all cache refs for the current cache version
    let cache_refspec = format!(
        "+refs/josh/cache/{}/*:refs/josh/cache/{}/*",
        CACHE_VERSION, CACHE_VERSION
    );
    spawn_git_command(repo.path(), &["push", &url, &cache_refspec], &[])
        .context("Failed to push cache refs")?;

    // Push filtered refs for the default branch only so that recipients have
    // all intermediate git objects needed to use the cache.
    for step_idx in 0..steps.len() {
        let prefix_path = step_ref_prefix(step_idx, &steps);
        let local_ref = format!(
            "refs/josh/filtered/{}/heads/{}",
            prefix_path, default_branch
        );

        if repo.find_reference(&local_ref).is_err() {
            eprintln!(
                "Warning: filtered ref '{}' not found — run 'josh cache build' first",
                local_ref
            );
            continue;
        }

        let filtered_refspec = format!("+{r}:{r}", r = local_ref);
        spawn_git_command(repo.path(), &["push", &url, &filtered_refspec], &[])
            .with_context(|| format!("Failed to push filtered ref for step {}", step_idx))?;
    }

    eprintln!(
        "Pushed cache for remote '{}' (filter: {})",
        args.remote,
        &filter.id().to_string()[..8]
    );
    Ok(())
}

/// Fetch cache refs from `url` into the local repo.
///
/// - `refs/josh/cache/{VERSION}/*` is fetched; errors are surfaced to the caller.
/// - `refs/josh/filtered/…` per-step refs are fetched best-effort (errors ignored).
pub fn fetch_remote_cache(
    repo: &git2::Repository,
    url: &str,
    filter: Filter,
) -> anyhow::Result<()> {
    let cache_refspec = format!(
        "+refs/josh/cache/{v}/*:refs/josh/cache/{v}/*",
        v = CACHE_VERSION
    );
    spawn_git_command(repo.path(), &["fetch", url, &cache_refspec], &[])
        .context("Failed to fetch cache refs")?;

    let steps = flatten_chain(filter);
    for step_idx in 0..steps.len() {
        let prefix_path = step_ref_prefix(step_idx, &steps);
        let filtered_refspec = format!(
            "+refs/josh/filtered/{p}/heads/*:refs/josh/filtered/{p}/heads/*",
            p = prefix_path
        );
        // Ignore errors: the remote may not have filtered refs for this step yet
        let _ = spawn_git_command(repo.path(), &["fetch", url, &filtered_refspec], &[]);
    }

    Ok(())
}

fn handle_cache_fetch(args: &CacheFetchArgs, transaction: &Transaction) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let RemoteConfig {
        url,
        filter_with_meta,
        ..
    } = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    let filter = filter_with_meta.peel();

    fetch_remote_cache(repo, &url, filter)?;

    eprintln!(
        "Fetched cache for remote '{}' (filter: {})",
        args.remote,
        &filter.id().to_string()[..8]
    );
    Ok(())
}
