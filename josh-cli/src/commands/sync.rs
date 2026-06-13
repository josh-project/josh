use crate::commands::scope::ScopeArgs;

/// Arguments for `josh changes sync`.
#[derive(Debug, clap::Parser)]
pub struct SyncArgs {
    #[command(flatten)]
    pub scope: ScopeArgs,

    /// Discard existing refs/josh/changes (for the resolved scope kind) before syncing.
    #[arg(long = "clean")]
    pub clean: bool,

    /// Push outbox comments and votes to GitHub (Remote scope only).
    #[arg(long = "push")]
    pub push: bool,

    /// Fetch all PR data fresh, ignoring the sync fingerprint cache.
    #[arg(long = "no-cache")]
    pub no_cache: bool,

    /// Seconds a sync fingerprint stays valid before forcing a refetch.
    #[arg(long = "cache-ttl", default_value_t = 3600)]
    pub cache_ttl: u64,
}

pub fn handle_sync(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_core::filter::check_experimental_features_enabled("josh changes sync")?;
    let scope = args.scope.resolve(transaction)?;
    let opts = josh_github_changes::sync::SyncOptions {
        clean: args.clean,
        push: args.push,
        no_cache: args.no_cache,
        cache_ttl: args.cache_ttl,
    };

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(josh_github_changes::sync::sync(transaction, &scope, opts))?;

    Ok(())
}
