/// Selects which `refs/josh/...` changes ref a subcommand operates on.
///
/// Default (no `--remote`) targets the Local ref for the chosen branch;
/// passing `--remote <name>` targets that remote's changes ref. Branch
/// defaults to the current HEAD's branch.
#[derive(Debug, Clone, clap::Args)]
pub struct ScopeArgs {
    /// Target branch (default: HEAD's branch).
    #[arg(short = 'b', long = "branch")]
    pub branch: Option<String>,

    /// Operate on the changes ref for this remote instead of the Local one.
    #[arg(long = "remote")]
    pub remote: Option<String>,
}

impl ScopeArgs {
    pub fn resolve(
        &self,
        transaction: &josh_core::cache::Transaction,
    ) -> anyhow::Result<josh_changes::ChangesRef> {
        josh_changes::ChangesRef::resolve(
            transaction,
            self.branch.as_deref(),
            self.remote.as_deref(),
        )
    }
}
