use anyhow::Context;
use clap::Parser;

use josh_cli::commands::auth::AuthArgs;
use josh_cli::commands::cache::CacheArgs;
use josh_cli::commands::link::LinkArgs;
use josh_cli::commands::push::{PublishArgs, PushArgs};
use josh_cli::config::{RemoteConfig, read_remote_config, write_remote_config};
use josh_cli::forge::Forge;
use josh_core::git::{normalize_repo_path, spawn_git_command};

#[derive(Debug, clap::Parser)]
#[command(
    name = "josh",
    version = josh_core::VERSION,
    about = "Josh: Git projections & sync tooling",
    long_about = None,
)]
pub struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    #[command(flatten)]
    Repo(RepoCommand),
    #[command(flatten)]
    Standalone(StandaloneCommand),
}

/// Commands that require a git repository and transaction context
#[derive(Debug, clap::Subcommand)]
pub enum RepoCommand {
    /// Clone a repository with optional projection/filtering
    Clone(CloneArgs),

    /// Fetch from a remote (like `git fetch`) with projection-aware options
    Fetch(FetchArgs),

    /// Fetch & integrate from a remote (like `git pull`) with projection-aware options
    Pull(PullArgs),

    /// Push refs to a remote (like `git push`) with projection-aware options
    Push(PushArgs),

    /// Push each commit as an independent, minimal diff (stacked changes workflow)
    Publish(PublishArgs),

    /// Add a remote with optional projection/filtering (like `git remote add`)
    Remote(RemoteArgs),

    /// Apply filtering to existing refs (like `josh fetch` but without fetching)
    Filter(FilterArgs),

    /// Manage josh links (like `josh remote` but for links)
    Link(LinkArgs),

    /// Manage the distributed filter cache
    Cache(CacheArgs),
}

/// Commands that don't require a git repository
#[derive(Debug, clap::Subcommand)]
pub enum StandaloneCommand {
    /// Manage forge authentication
    Auth(AuthArgs),
}

#[derive(Debug, clap::Parser)]
pub struct CloneArgs {
    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Workspace/projection identifier or path to spec
    #[arg()]
    pub filter: String,

    /// Checkout directory
    #[arg()]
    pub out: std::path::PathBuf,

    /// Branch or ref to clone
    #[arg(short = 'b', long = "branch", default_value = "HEAD")]
    pub branch: String,

    #[command(flatten)]
    pub forge_args: ForgeArgs,
}

#[derive(Debug, clap::Parser)]
pub struct PullArgs {
    /// Remote name (or URL) to pull from
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// Ref to pull (branch, tag, or commit-ish)
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,

    /// Rebase the current branch on top of the upstream branch
    #[arg(long = "rebase", action = clap::ArgAction::SetTrue)]
    pub rebase: bool,

    /// Automatically stash local changes before rebase
    #[arg(long = "autostash", action = clap::ArgAction::SetTrue)]
    pub autostash: bool,
}

#[derive(Debug, clap::Parser)]
pub struct FetchArgs {
    /// Remote name (or URL) to fetch from
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// Ref to fetch (branch, tag, or commit-ish)
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,
}

#[derive(Debug, clap::Parser)]
pub struct RemoteArgs {
    /// Remote subcommand
    #[command(subcommand)]
    pub command: RemoteCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum RemoteCommand {
    /// Add a remote with optional projection/filtering
    Add(RemoteAddArgs),
}

#[derive(Debug, clap::Parser)]
pub struct RemoteAddArgs {
    /// Remote name
    #[arg()]
    pub name: String,

    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Workspace/projection identifier or path to spec
    #[arg()]
    pub filter: String,

    #[command(flatten)]
    pub forge_args: ForgeArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ForgeArgs {
    /// Forge type for the remote (e.g. github)
    #[arg(long = "forge", value_enum, conflicts_with = "no_forge")]
    pub forge: Option<Forge>,

    /// Explicitly disable forge integration
    #[arg(long = "no-forge", conflicts_with = "forge")]
    pub no_forge: bool,
}

#[derive(Debug, clap::Parser)]
pub struct FilterArgs {
    /// Remote name to apply filtering to
    #[arg()]
    pub remote: String,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Standalone(cmd) => run_standalone(cmd),
        Command::Repo(cmd) => run_repo(cmd),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");

        for e in e.chain() {
            eprintln!("{e}");
        }

        std::process::exit(1);
    }
}

fn run_standalone(cmd: &StandaloneCommand) -> anyhow::Result<()> {
    match cmd {
        StandaloneCommand::Auth(args) => josh_cli::commands::auth::handle_auth(args),
    }
}

fn run_repo(cmd: &RepoCommand) -> anyhow::Result<()> {
    // For clone, do the initial repo setup before creating transaction
    let repo_path = if let RepoCommand::Clone(args) = cmd {
        // For clone, we're not in a git repo initially, so clone first and use that path
        clone_repo(args)?
    } else {
        // For other commands, we need to be in a git repo
        let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
        normalize_repo_path(repo.path())
    };

    josh_core::cache::sled_load(&repo_path.join(".git")).context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default())
            .with_backend(
                josh_core::cache::DistributedCacheBackend::new(&repo_path)
                    .context("Failed to create DistributedCacheBackend")?,
            ),
    );

    // Create transaction using the known repo path
    let transaction = josh_core::cache::TransactionContext::new(&repo_path, cache.clone())
        .open(None)
        .context("Failed TransactionContext::open")?;

    match cmd {
        RepoCommand::Clone(args) => handle_clone(args, &transaction),
        RepoCommand::Fetch(args) => handle_fetch(args, &transaction),
        RepoCommand::Pull(args) => handle_pull(args, &transaction),
        RepoCommand::Push(args) => josh_cli::commands::push::handle_push(args, &transaction),
        RepoCommand::Publish(args) => josh_cli::commands::push::handle_publish(args, &transaction),
        RepoCommand::Remote(args) => handle_remote(args, &transaction),
        RepoCommand::Filter(args) => handle_filter(args, &transaction),
        RepoCommand::Link(args) => josh_cli::commands::link::handle_link(args, &transaction),
        RepoCommand::Cache(args) => josh_cli::commands::cache::handle_cache(args, &transaction),
    }
}

fn to_absolute_remote_url(url: &str) -> anyhow::Result<String> {
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ssh://")
        || url.starts_with("file://")
    {
        Ok(url.to_owned())
    } else {
        // For local paths, make them absolute
        let path = std::fs::canonicalize(url)
            .with_context(|| format!("Failed to resolve path {}", url))?
            .display()
            .to_string();

        Ok(format!("file://{}", path))
    }
}

/// Initial clone setup: create directory, init repo, add remote (no transaction needed)
fn clone_repo(args: &CloneArgs) -> anyhow::Result<std::path::PathBuf> {
    // Use the provided output directory
    let output_dir = args.out.clone();

    // Create the output directory first
    std::fs::create_dir_all(&output_dir)?;

    // Initialize a new git repository inside the directory using git2
    git2::Repository::init(&output_dir).context("Failed to initialize git repository")?;

    // Use handle_remote_add to add the remote with the filter
    let remote_add_args = RemoteAddArgs {
        name: "origin".to_string(),
        url: to_absolute_remote_url(&args.url)?,
        filter: args.filter.clone(),
        forge_args: args.forge_args.clone(),
    };

    handle_remote_add_repo(&remote_add_args, &output_dir)?;

    Ok(output_dir)
}

fn handle_clone(
    args: &CloneArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Create FetchArgs from CloneArgs
    let fetch_args = FetchArgs {
        remote: "origin".to_string(),
        rref: args.branch.clone(),
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch(&fetch_args, transaction)?;

    // Get the default branch name from the remote HEAD symref
    let default_branch = if args.branch == "HEAD" {
        // Read the remote HEAD symref to get the default branch
        let head_ref = "refs/remotes/origin/HEAD".to_string();

        let head_reference = repo
            .find_reference(&head_ref)
            .with_context(|| format!("Failed to find remote HEAD reference {}", head_ref))?;

        let symref_target = head_reference
            .symbolic_target()
            .context("Remote HEAD reference is not a symbolic reference")?;

        // Extract branch name from symref target (e.g., "refs/remotes/origin/master" -> "master")
        let branch_name = symref_target
            .strip_prefix("refs/remotes/origin/")
            .with_context(|| format!("Invalid symref target format: {}", symref_target))?;

        branch_name.to_string()
    } else {
        args.branch.clone()
    };

    spawn_git_command(
        repo.path(),
        &[
            "checkout",
            "-b",
            &default_branch,
            &format!("origin/{}", default_branch),
        ],
        &[],
    )
    .with_context(|| format!("Failed to checkout branch {}", default_branch))?;

    // Set up upstream tracking for the branch
    spawn_git_command(
        repo.path(),
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", default_branch),
            &default_branch,
        ],
        &[],
    )
    .with_context(|| format!("Failed to set upstream for branch {}", default_branch))?;

    let output_dir = normalize_repo_path(repo.path());
    let output_dir = output_dir.display().to_string();

    let output_dir = if let Ok(testtmp) = std::env::var("TESTTMP") {
        output_dir.replace(&testtmp, "${TESTTMP}")
    } else {
        output_dir.to_string()
    };

    println!("Cloned repository to: {}", output_dir);
    Ok(())
}

fn handle_pull(args: &PullArgs, transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Create FetchArgs from PullArgs
    let fetch_args = FetchArgs {
        remote: args.remote.clone(),
        rref: args.rref.clone(),
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch(&fetch_args, transaction)?;

    // Now use actual git pull to integrate the changes
    let mut git_args = vec!["pull"];

    if args.rebase {
        git_args.push("--rebase");
    }

    if args.autostash {
        git_args.push("--autostash");
    }

    git_args.push(&args.remote);

    spawn_git_command(repo.path(), &git_args, &[]).context("git pull failed")?;

    eprintln!("Pulled from remote: {}", args.remote);

    Ok(())
}

fn try_parse_symref(remote: &str, output: &str) -> Option<(String, String)> {
    josh_cli::remote_ops::try_parse_symref(remote, output)
}

fn handle_fetch(
    args: &FetchArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    // Read the remote configuration from .git/josh/remotes/<name>.josh
    let RemoteConfig {
        url,
        ref_spec,
        filter_with_meta,
        ..
    } = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    // First, fetch unfiltered refs to refs/josh/remotes/*
    spawn_git_command(repo.path(), &["fetch", &url, &ref_spec], &[])
        .context("git fetch to josh/remotes failed")?;

    // Warm the local cache from the remote before filtering
    let filter = filter_with_meta.peel();
    if let Err(e) = josh_cli::commands::cache::fetch_remote_cache(repo, &url, filter) {
        eprintln!("Warning: could not fetch remote cache: {e}");
    }

    // Set up remote HEAD reference using git ls-remote
    // This is the proper way to get the default branch from the remote
    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);

    // Use git ls-remote --symref to get the default branch
    // Parse the output to get the default branch name
    // Output format: "ref: refs/heads/main\t<commit-hash>"
    let output = std::process::Command::new("git")
        .args(["ls-remote", "--symref", &url, "HEAD"])
        .current_dir(normalize_repo_path(repo.path()))
        .output()?;

    if output.status.success() {
        let output = String::from_utf8(output.stdout)?;

        if let Some((default_branch, default_branch_ref)) = try_parse_symref(&args.remote, &output)
        {
            repo.reference_symbolic(&head_ref, &default_branch_ref, true, "josh remote HEAD")?;

            repo.reference_symbolic(
                &format!("refs/namespaces/josh-{}/{}", args.remote, "HEAD"),
                &format!("refs/heads/{}", default_branch),
                true,
                "josh remote HEAD",
            )?;
        }
    }

    josh_cli::remote_ops::apply_josh_filtering(transaction, &repo_path, filter, &args.remote)?;

    // Note: fetch doesn't checkout, it just updates the refs
    eprintln!("Fetched from remote: {}", args.remote);

    Ok(())
}

fn handle_remote(
    args: &RemoteArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        RemoteCommand::Add(add_args) => {
            let repo_path = normalize_repo_path(transaction.repo().path());
            handle_remote_add_repo(add_args, &repo_path)
        }
    }
}

fn handle_remote_add_repo(args: &RemoteAddArgs, repo_path: &std::path::Path) -> anyhow::Result<()> {
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;
    let workdir = normalize_repo_path(repo_path);

    // Store the remote information in .git/josh/remotes/<name>.josh file
    let remote_url = to_absolute_remote_url(&args.url)?;

    // Store the filter in git config per remote
    let filter_to_store = args.filter.clone();

    // Store refspec (for unfiltered refs)
    let refspec = format!("+refs/heads/*:refs/josh/remotes/{}/*", args.name);

    let forge = if args.forge_args.no_forge {
        None
    } else {
        args.forge_args
            .forge
            .clone()
            .or_else(|| josh_cli::forge::guess_forge(&remote_url))
    };

    // Write remote config to .git/josh/remotes/<name>.josh
    write_remote_config(
        repo_path,
        &args.name,
        &remote_url,
        &filter_to_store,
        &refspec,
        forge,
    )
    .context("Failed to write remote config file")?;

    // Set up a git remote that points to "." with a refspec to fetch filtered refs
    // Add remote pointing to current directory
    let repo_remote = to_absolute_remote_url(&workdir.display().to_string())?;
    spawn_git_command(
        repo.path(),
        &["remote", "add", &args.name, &repo_remote],
        &[],
    )
    .context("Failed to add git remote")?;

    // Set up namespace configuration for the remote
    let namespace = format!("josh-{}", args.name);
    let uploadpack_cmd = format!("env GIT_NAMESPACE={} git upload-pack", namespace);

    spawn_git_command(
        repo.path(),
        &[
            "config",
            &format!("remote.{}.uploadpack", args.name),
            &uploadpack_cmd,
        ],
        &[],
    )
    .context("Failed to set remote uploadpack")?;

    eprintln!(
        "Added remote '{}' with filter '{}'",
        args.name, filter_to_store
    );

    Ok(())
}

/// Handle the `josh filter` command - apply filtering to existing refs without fetching
fn handle_filter(
    args: &FilterArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let RemoteConfig {
        filter_with_meta, ..
    } = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    let filter = filter_with_meta.peel();
    let filter_str = josh_core::filter::spec(filter);

    println!(
        "Applying filter '{}' to remote '{}'",
        filter_str, args.remote
    );

    josh_cli::remote_ops::apply_josh_filtering(transaction, &repo_path, filter, &args.remote)?;

    println!(
        "Applied filter '{}' to remote '{}'",
        filter_str, args.remote
    );

    Ok(())
}
