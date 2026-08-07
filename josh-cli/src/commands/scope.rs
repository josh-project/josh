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
    pub fn resolve(&self, repo: &git2::Repository) -> anyhow::Result<josh_changes::ChangesRef> {
        let branch = match &self.branch {
            Some(b) => b.clone(),
            None => josh_changes::head_branch(repo)?,
        };
        Ok(match &self.remote {
            Some(name) => josh_changes::ChangesRef::Remote {
                remote: name.clone(),
                branch,
            },
            None => josh_changes::ChangesRef::Local { branch },
        })
    }
}
