use std::collections::HashSet;

use anyhow::Context;

use josh_core::cache::{
    CACHE_VERSION, CacheStack, DistributedCacheBackend, Transaction, TransactionContext,
};
use josh_core::filter::{self, Filter, flatten_chain, from_tree};
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

fn handle_cache_build(args: &CacheBuildArgs, transaction: &Transaction) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let default_branch = remote_ops::resolve_default_branch(repo, &args.remote)?;

    // Discover known filter chains from refs/josh/filtered/ refs.
    let mut chain_prefixes: HashSet<String> = HashSet::new();
    if let Ok(refs) = repo.references_glob("refs/josh/filtered/*") {
        for reference in refs {
            let reference = match reference {
                Ok(r) => r,
                Err(_) => continue,
            };
            let refname = match reference.name() {
                Some(n) => n,
                None => continue,
            };
            if let Some(rest) = refname.strip_prefix("refs/josh/filtered/") {
                if let Some(heads_pos) = rest.find("/heads/") {
                    chain_prefixes.insert(rest[..heads_pos].to_string());
                }
            }
        }
    }

    // Keep only the longest prefixes (full chains).
    // A shorter prefix like "B/A" is an intermediate step of "C/B/A".
    let full_chains: Vec<String> = {
        let mut sorted: Vec<_> = chain_prefixes.iter().cloned().collect();
        sorted.sort();
        sorted
            .iter()
            .filter(|p| {
                !sorted
                    .iter()
                    .any(|other| other != *p && other.ends_with(&format!("/{}", p)))
            })
            .cloned()
            .collect()
    };

    // Reconstruct filter step lists from chain prefixes.
    let mut all_step_lists: Vec<Vec<Filter>> = Vec::new();
    for chain_prefix in &full_chains {
        let ids: Vec<&str> = chain_prefix.split('/').collect();
        // IDs are newest-first in the path; reverse to get application order
        let mut steps = Vec::new();
        let mut ok = true;
        for id_str in ids.iter().rev() {
            match git2::Oid::from_str(id_str)
                .map_err(anyhow::Error::from)
                .and_then(|oid| from_tree(repo, oid))
            {
                Ok(f) => steps.push(f),
                Err(e) => {
                    eprintln!(
                        "Warning: could not reconstruct filter from '{}': {}",
                        id_str, e
                    );
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            all_step_lists.push(steps);
        }
    }

    // Add the config filter as fallback if not already discovered.
    if let Ok(config) = read_remote_config(&repo_path, &args.remote) {
        let config_filter = config.filter_with_meta.peel();
        let config_steps = flatten_chain(config_filter);
        let config_prefix = remote_ops::step_ref_prefix(config_steps.len() - 1, &config_steps);
        if !full_chains.contains(&config_prefix) {
            all_step_lists.push(config_steps);
        }
    }

    if all_step_lists.is_empty() {
        return Err(anyhow::anyhow!(
            "No known filters found and no remote config for '{}'",
            args.remote
        ));
    }

    // Build a transaction that has ONLY the DistributedCacheBackend — no Sled.
    // This ensures every filter operation writes a new entry to the distributed
    // cache even if Sled already has a result for that commit.
    let cache = std::sync::Arc::new(CacheStack::new().with_backend(
        DistributedCacheBackend::new(&repo_path).context("Failed to open distributed cache")?,
    ));
    let build_transaction = TransactionContext::new(&repo_path, cache)
        .open(None)
        .context("Failed to open build transaction")?;

    // Resolve the default branch commit from backing refs.
    let backing_prefix = format!("refs/josh/remotes/{}/", args.remote);
    let default_branch_ref = format!("{}{}", backing_prefix, default_branch);
    let default_branch_oid = build_transaction
        .repo()
        .find_reference(&default_branch_ref)
        .with_context(|| {
            format!(
                "Default branch '{}' not found in refs/josh/remotes/{}/",
                default_branch, args.remote
            )
        })?
        .target()
        .context("Default branch ref has no target")?;

    let seed_commits = vec![(default_branch.clone(), default_branch_oid)];

    // Build cache for each discovered filter chain.
    for steps in &all_step_lists {
        let mut current_commits = seed_commits.clone();

        for (step_idx, step_filter) in steps.iter().enumerate() {
            let (filtered, errors) =
                josh_core::filter_refs(&build_transaction, *step_filter, &current_commits);

            if let Some(error) = errors.into_iter().next() {
                eprintln!(
                    "Warning: filter error for {}: {}",
                    filter::spec(*step_filter),
                    error.1
                );
                break;
            }

            let prefix_path = remote_ops::step_ref_prefix(step_idx, steps);
            let mut next_commits = Vec::new();

            for (branch_name, filtered_oid) in filtered {
                if filtered_oid == git2::Oid::zero() {
                    continue;
                }
                let filtered_ref =
                    format!("refs/josh/filtered/{}/heads/{}", prefix_path, branch_name);
                repo.reference(&filtered_ref, filtered_oid, true, "josh cache build")
                    .with_context(|| format!("failed to write filtered ref '{}'", filtered_ref))?;
                next_commits.push((branch_name, filtered_oid));
            }

            current_commits = next_commits;
        }
    }

    eprintln!(
        "Built cache for {} filter(s) on branch '{}' for remote '{}'",
        all_step_lists.len(),
        default_branch,
        args.remote
    );
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

    let default_branch = remote_ops::resolve_default_branch(repo, &args.remote)?;

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
        let prefix_path = remote_ops::step_ref_prefix(step_idx, &steps);
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
        let prefix_path = remote_ops::step_ref_prefix(step_idx, &steps);
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
