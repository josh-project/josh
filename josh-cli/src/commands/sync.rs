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
}

pub fn handle_sync(
    args: &SyncArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let scope = args.scope.resolve(repo)?;
    let opts = josh_github_changes::sync::SyncOptions {
        clean: args.clean,
        push: args.push,
    };

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(josh_github_changes::sync::sync(
        repo,
        transaction,
        &scope,
        opts,
    ))?;

    Ok(())
}
