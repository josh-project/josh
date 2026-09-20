//! Hidden `josh forge` command: the only writer of test-forge state.
//!
//! The test forge's server-side state (CI checks, reviews, maintainers,
//! admission rules) lives in a ref in the *remote* repository; these
//! subcommands resolve a remote from the current repo's config, require it to
//! be a test-forge remote, open the remote repository, and write the state
//! there. Test-suite plumbing for exercising `josh changes sync` without
//! GitHub; hidden from help and gated on `JOSH_EXPERIMENTAL_FEATURES`.

use anyhow::{Context, anyhow};
use clap::Subcommand;

use josh_test_changes::{CheckState, PullRequestReviewState};

const CHECK_STATES: [&str; 8] = [
    "pending",
    "success",
    "failure",
    "neutral",
    "skipped",
    "cancelled",
    "timed_out",
    "action_required",
];

const REVIEW_STATES: [&str; 4] = ["approved", "changes_requested", "commented", "dismissed"];

#[derive(Debug, clap::Parser)]
pub struct ForgeArgs {
    /// Remote whose test-forge state to modify
    #[arg(long = "remote")]
    pub remote: String,

    /// State to modify
    #[command(subcommand)]
    pub command: ForgeCommand,
}

#[derive(Debug, Subcommand)]
pub enum ForgeCommand {
    /// Set CI check results for a commit
    Check(CheckArgs),
    /// Record a review on a change
    Review(ReviewArgs),
    /// Set admission requirements for a target branch
    Admission(AdmissionArgs),
    /// Manage the maintainer set
    Maintainer(MaintainerArgs),
}

#[derive(Debug, clap::Parser)]
pub struct CheckArgs {
    #[command(subcommand)]
    pub command: CheckCommand,
}

#[derive(Debug, Subcommand)]
pub enum CheckCommand {
    /// Set the state of a check on a commit
    Set(CheckSetArgs),
}

#[derive(Debug, clap::Parser)]
pub struct CheckSetArgs {
    /// Commit the check ran on
    #[arg()]
    pub oid: String,

    /// Check name
    #[arg()]
    pub name: String,

    /// Check state
    #[arg()]
    pub state: String,
}

#[derive(Debug, clap::Parser)]
pub struct ReviewArgs {
    /// Change to review
    #[arg()]
    pub change_id: String,

    /// Reviewing user
    #[arg()]
    pub user: String,

    /// Review state
    #[arg()]
    pub state: String,
}

#[derive(Debug, clap::Parser)]
pub struct AdmissionArgs {
    #[command(subcommand)]
    pub command: AdmissionCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdmissionCommand {
    /// Replace the branch's admission requirements wholesale: the stored
    /// required-checks list becomes exactly the given --require-check values
    /// (pass none to clear the list) and the approvals bar becomes
    /// --required-approvals (0 = floor of one maintainer approval)
    Set(AdmissionSetArgs),
}

#[derive(Debug, clap::Parser)]
pub struct AdmissionSetArgs {
    /// Target branch the requirements apply to
    #[arg(long = "branch")]
    pub branch: String,

    /// Required check (repeatable)
    #[arg(long = "require-check")]
    pub require_checks: Vec<String>,

    /// Required approving reviews
    #[arg(long = "required-approvals", default_value_t = 0)]
    pub required_approvals: u32,
}

#[derive(Debug, clap::Parser)]
pub struct MaintainerArgs {
    #[command(subcommand)]
    pub command: MaintainerCommand,
}

#[derive(Debug, Subcommand)]
pub enum MaintainerCommand {
    /// Add a maintainer
    Add(MaintainerUserArgs),
    /// Remove a maintainer
    Remove(MaintainerUserArgs),
}

#[derive(Debug, clap::Parser)]
pub struct MaintainerUserArgs {
    /// Maintainer login
    #[arg()]
    pub user: String,
}

pub fn handle_forge(
    args: &ForgeArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_core::filter::check_experimental_features_enabled("josh forge")?;

    let repo_path = josh_core::git::normalize_repo_path(transaction.path());
    let config = josh_changes::remote_config::read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;
    anyhow::ensure!(
        config.forge == Some(crate::forge::Forge::Test),
        "remote '{}' is not a test-forge remote (forge: {})",
        args.remote,
        config
            .forge
            .map(|forge| forge.to_string())
            .unwrap_or_else(|| "none".to_string()),
    );

    let remote_path = remote_repo_path(&config.url)?;
    let remote = josh_core::cache::TransactionContext::new(
        &remote_path,
        std::sync::Arc::new(josh_core::cache::CacheStack::new()),
    )
    .open()
    .with_context(|| format!("Failed to open remote repository {}", remote_path.display()))?;

    match &args.command {
        ForgeCommand::Check(check_args) => match &check_args.command {
            CheckCommand::Set(set_args) => {
                let oid = gix_hash::ObjectId::from_hex(set_args.oid.as_bytes())
                    .map_err(|_| anyhow!("invalid commit oid '{}'", set_args.oid))?;
                let state = CheckState::from_str(&set_args.state).ok_or_else(|| {
                    anyhow!(
                        "unknown check state '{}' (valid: {})",
                        set_args.state,
                        CHECK_STATES.join(", ")
                    )
                })?;
                josh_test_changes::set_check(&remote, oid, &set_args.name, state)?;
                println!(
                    "Set check '{}' on {} to {}",
                    set_args.name,
                    set_args.oid,
                    state.as_str()
                );
            }
        },
        ForgeCommand::Review(review_args) => {
            let state = PullRequestReviewState::from_str(&review_args.state).ok_or_else(|| {
                anyhow!(
                    "unknown review state '{}' (valid: {})",
                    review_args.state,
                    REVIEW_STATES.join(", ")
                )
            })?;
            josh_test_changes::set_review(
                &remote,
                &review_args.change_id,
                &review_args.user,
                state.clone(),
            )?;
            println!(
                "Recorded review by {} on change '{}': {}",
                review_args.user,
                review_args.change_id,
                state.as_str()
            );
        }
        ForgeCommand::Admission(admission_args) => match &admission_args.command {
            AdmissionCommand::Set(set_args) => {
                josh_test_changes::set_admission(
                    &remote,
                    &set_args.branch,
                    &set_args.require_checks,
                    set_args.required_approvals,
                )?;
                println!(
                    "Set admission for branch '{}': required checks [{}], required approvals {}",
                    set_args.branch,
                    set_args.require_checks.join(", "),
                    set_args.required_approvals
                );
            }
        },
        ForgeCommand::Maintainer(maintainer_args) => match &maintainer_args.command {
            MaintainerCommand::Add(user_args) => {
                josh_test_changes::add_maintainer(&remote, &user_args.user)?;
                println!("Added maintainer '{}'", user_args.user);
            }
            MaintainerCommand::Remove(user_args) => {
                josh_test_changes::remove_maintainer(&remote, &user_args.user)?;
                println!("Removed maintainer '{}'", user_args.user);
            }
        },
    }

    Ok(())
}

/// The remote's URL as a local filesystem path. Test-forge remotes are local
/// (bare) repositories, so anything with a non-file scheme is rejected.
fn remote_repo_path(url: &str) -> anyhow::Result<std::path::PathBuf> {
    match url::Url::parse(url) {
        Ok(parsed) => {
            anyhow::ensure!(
                parsed.scheme() == "file",
                "test-forge remote URL must be a local path, got '{}'",
                url
            );
            parsed
                .to_file_path()
                .map_err(|_| anyhow!("cannot convert file URL to a path: '{}'", url))
        }
        Err(_) => Ok(std::path::PathBuf::from(url)),
    }
}
