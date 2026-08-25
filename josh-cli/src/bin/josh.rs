use anyhow::Context;
use clap::Parser;

use josh_cli::commands::auth::AuthArgs;
use josh_cli::commands::cache::CacheArgs;
use josh_cli::commands::changes::{DepsArgs, ListArgs, ShowArgs};
use josh_cli::commands::comment::CommentArgs;
use josh_cli::commands::fetch::FetchArgs;
use josh_cli::commands::link::LinkArgs;
use josh_cli::commands::pull::PullArgs;
use josh_cli::commands::push::{PublishArgs, PushArgs};
use josh_cli::commands::run::ComposeArgs;
use josh_cli::commands::sync::SyncArgs;
use josh_cli::config::{read_remote_config, write_remote_config};
use josh_cli::forge::{Forge, GerritMode};
use josh_core::git::{GitCommand, normalize_repo_path};

#[derive(Debug, clap::Parser)]
#[command(
    name = "josh",
    version = josh_core::VERSION,
    about = "Josh: Git projections & sync tooling",
    long_about = None,
)]
pub struct Cli {
    /// Disable the distributed filter cache (don't read, write or fetch it)
    #[arg(long = "no-distributed-cache", action = clap::ArgAction::SetFalse, global = true)]
    pub distributed_cache: bool,

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

    /// Push refs to a remote (like `git push`) with projection-aware options
    Push(PushArgs),

    /// Manage stacked changes (publish, etc.)
    Changes(ChangesArgs),

    /// Add a remote with optional projection/filtering (like `git remote add`)
    Remote(RemoteArgs),

    /// Apply filtering to existing refs (like `josh fetch` but without fetching)
    Filter(FilterArgs),

    /// Manage josh links (like `josh remote` but for links)
    Link(LinkArgs),

    /// Manage the distributed filter cache
    Cache(CacheArgs),

    /// Run workspaces in containers
    Compose(ComposeArgs),
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

    /// Separate push destination (a fork) for `josh changes publish`.
    ///
    /// See `josh remote add --push-url`.
    #[arg(long = "push-url")]
    pub push_url: Option<String>,

    #[command(flatten)]
    pub forge_args: ForgeArgs,
}

#[derive(Debug, clap::Parser)]
pub struct ChangesArgs {
    #[command(subcommand)]
    pub command: ChangesCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ChangesCommand {
    /// Push each commit as an independent, minimal diff (stacked changes workflow)
    Publish(PublishArgs),
    /// Fetch & integrate from a remote, rebase-style with autostash (stacked changes workflow)
    Pull(PullArgs),
    /// List stored changes with a one-line summary per change
    List(ListArgs),
    /// Print full detail for a change, including comments
    Show(ShowArgs),
    /// Print the change-ids this change depends on
    Deps(DepsArgs),
    /// Add a comment to a change
    Comment(CommentArgs),
    /// Sync GitHub PR comments to local change comments
    Sync(SyncArgs),
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

    /// Separate push destination (a fork) for `josh changes publish`.
    ///
    /// When set, change branches are pushed here while the main URL stays the
    /// fetch source and pull-request target. Pull requests (including upstack
    /// drafts) are opened against the main URL with a cross-fork head.
    #[arg(long = "push-url")]
    pub push_url: Option<String>,

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

    /// For a Gerrit remote, how `josh changes publish` maps the stack onto
    /// Gerrit changes: `independent` (default) publishes only dependency-free
    /// changes as separate reviews; `stack` pushes the whole history as one
    /// relation chain.
    #[arg(long = "gerrit-mode", value_enum)]
    pub gerrit_mode: Option<GerritMode>,
}

#[derive(Debug, clap::Parser)]
pub struct FilterArgs {
    /// Remote name to apply filtering to
    #[arg()]
    pub remote: String,
}

fn main() -> std::process::ExitCode {
    let _flush_guard = josh_core::memodb::FlushGuard::new();
    env_logger::init();
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Standalone(cmd) => run_standalone(cmd),
        Command::Repo(cmd) => run_repo(cmd, cli.distributed_cache),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");

        for e in e.chain() {
            eprintln!("{e}");
        }
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn run_standalone(cmd: &StandaloneCommand) -> anyhow::Result<()> {
    match cmd {
        StandaloneCommand::Auth(args) => josh_cli::commands::auth::handle_auth(args),
    }
}

fn run_repo(cmd: &RepoCommand, distributed_cache: bool) -> anyhow::Result<()> {
    let (repo_path, git_common_dir) = if let RepoCommand::Clone(args) = cmd {
        // For clone, we're not in a git repo initially, so clone first.
        let repo = gix::open(clone_repo(args)?)?;
        (
            repo.workdir().unwrap_or_else(|| repo.git_dir()).to_owned(),
            repo.common_dir().to_owned(),
        )
    } else {
        let paths =
            josh_core::git::discover_repository_paths().context("Not in a git repository")?;
        (paths.workdir_or_gitdir, paths.common_dir)
    };

    let is_compose = matches!(cmd, RepoCommand::Compose(_));

    let mut cache_stack = josh_core::cache::CacheStack::new();
    // Compose does one-shot, throwaway filtering and then hands off to a long container run; the
    // on-disk sled cache would only take a lock we would have to release again, so skip it.
    if !is_compose {
        cache_stack =
            cache_stack.with_backend(josh_core::cache::SledCacheBackend::new(&git_common_dir));
    }
    if distributed_cache {
        cache_stack = cache_stack.with_backend(
            josh_core::cache::DistributedCacheBackend::new(&git_common_dir)
                .context("Failed to create DistributedCacheBackend")?,
        );
    }
    let cache = std::sync::Arc::new(cache_stack);

    let mut ctx = josh_core::cache::TransactionContext::new(&repo_path, cache.clone());

    // For compose, we don't need to flush the objects to disk;
    // everything else gets mem odb setup with an upper flush limit
    if is_compose {
        ctx = ctx.ephemeral();
    } else {
        ctx = ctx.with_mem_odb_limit(josh_cli::MAX_MEM_PACK_SIZE)
    }

    let transaction = ctx.open().context("Failed TransactionContext::open")?;

    match cmd {
        RepoCommand::Clone(args) => handle_clone(args, &transaction, distributed_cache),
        RepoCommand::Fetch(args) => {
            let remote = args.remote.clone();
            let updates =
                josh_cli::commands::fetch::handle_fetch(args, &transaction, distributed_cache)?;
            for line in josh_cli::commands::pull::render_fetch_summary(
                &updates,
                &remote,
                Some(&transaction),
            )? {
                eprintln!("{}", line);
            }
            eprintln!("Fetched from remote: {}", remote);
            Ok(())
        }
        RepoCommand::Push(args) => josh_cli::commands::push::handle_push(args, &transaction),
        RepoCommand::Changes(args) => match &args.command {
            ChangesCommand::Publish(publish_args) => {
                josh_cli::commands::push::handle_publish(publish_args, &transaction)?;
                let remote = publish_args.remote.as_deref().unwrap_or("origin");
                josh_cli::commands::fetch::handle_fetch(
                    &FetchArgs {
                        remote: remote.to_string(),
                        rref: "HEAD".to_string(),
                    },
                    &transaction,
                    distributed_cache,
                )
                .map(|_| ())
            }
            ChangesCommand::List(list_args) => {
                josh_cli::commands::changes::handle_list(list_args, &transaction)
            }
            ChangesCommand::Pull(pull_args) => {
                josh_cli::commands::pull::handle_pull(pull_args, &transaction, distributed_cache)
            }
            ChangesCommand::Show(show_args) => {
                josh_cli::commands::changes::handle_show(show_args, &transaction)
            }
            ChangesCommand::Deps(deps_args) => {
                josh_cli::commands::changes::handle_deps(deps_args, &transaction)
            }
            ChangesCommand::Comment(comment_args) => {
                josh_cli::commands::comment::handle_comment(comment_args, &transaction)
            }
            ChangesCommand::Sync(sync_args) => {
                josh_cli::commands::sync::handle_sync(sync_args, &transaction)
            }
        },
        RepoCommand::Remote(args) => handle_remote(args, &transaction),
        RepoCommand::Filter(args) => handle_filter(args, &transaction),
        RepoCommand::Link(args) => josh_cli::commands::link::handle_link(args, &transaction),
        RepoCommand::Compose(args) => josh_cli::commands::run::handle_compose(args, &transaction),
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
        // dunce, not std, on Windows: std::fs::canonicalize returns an extended-length
        // path (\\?\C:\...), which git rejects inside a file:// URL (issue #2288).
        #[cfg(windows)]
        let canonical = dunce::canonicalize(url);
        #[cfg(not(windows))]
        let canonical = std::fs::canonicalize(url);
        let canonical = canonical.with_context(|| format!("Failed to resolve path {}", url))?;

        let url = url::Url::from_file_path(&canonical).map_err(|_| {
            anyhow::anyhow!(
                "Path {} is not absolute or not convertible to a file URL",
                canonical.display()
            )
        })?;

        Ok(url.to_string())
    }
}

/// Initialize a clone and configure its remote.
fn clone_repo(args: &CloneArgs) -> anyhow::Result<std::path::PathBuf> {
    let output_dir = args.out.clone();

    std::fs::create_dir_all(&output_dir)?;

    gix::init(&output_dir).context("Failed to initialize git repository")?;

    let remote_add_args = RemoteAddArgs {
        name: "origin".to_string(),
        url: to_absolute_remote_url(&args.url)?,
        filter: args.filter.clone(),
        push_url: args.push_url.clone(),
        forge_args: args.forge_args.clone(),
    };

    handle_remote_add_repo(&remote_add_args, &output_dir)?;

    Ok(output_dir)
}

fn handle_clone(
    args: &CloneArgs,
    transaction: &josh_core::cache::Transaction,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    // Create FetchArgs from CloneArgs
    let fetch_args = FetchArgs {
        remote: "origin".to_string(),
        rref: args.branch.clone(),
    };

    // Use handle_fetch to do the actual fetching and filtering
    josh_cli::commands::fetch::handle_fetch(&fetch_args, transaction, distributed_cache)?;

    // Get the default branch name from the remote HEAD symref
    let default_branch = if args.branch == "HEAD" {
        // Read the remote HEAD symref to get the default branch
        let head_ref = "refs/remotes/origin/HEAD".to_string();

        let symref_target = transaction
            .symref_target(&head_ref)?
            .with_context(|| format!("{} is missing or not a symbolic reference", head_ref))?;

        // Extract branch name from symref target (e.g., "refs/remotes/origin/master" -> "master")
        let branch_name = symref_target
            .strip_prefix("refs/remotes/origin/")
            .with_context(|| format!("Invalid symref target format: {}", symref_target))?;

        branch_name.to_string()
    } else {
        args.branch.clone()
    };

    transaction
        .spawn_git(
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
    transaction
        .spawn_git(
            &[
                "branch",
                "--set-upstream-to",
                &format!("origin/{}", default_branch),
                &default_branch,
            ],
            &[],
        )
        .with_context(|| format!("Failed to set upstream for branch {}", default_branch))?;

    let output_dir = normalize_repo_path(transaction.path());
    let output_dir = output_dir.display().to_string();

    let output_dir = if let Ok(testtmp) = std::env::var("TESTTMP") {
        output_dir.replace(&testtmp, "${TESTTMP}")
    } else {
        output_dir.to_string()
    };

    println!("Cloned repository to: {}", output_dir);
    Ok(())
}

fn handle_remote(
    args: &RemoteArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        RemoteCommand::Add(add_args) => {
            let repo_path = normalize_repo_path(transaction.path());
            handle_remote_add_repo(add_args, &repo_path)
        }
    }
}

fn handle_remote_add_repo(args: &RemoteAddArgs, repo_path: &std::path::Path) -> anyhow::Result<()> {
    let repo = gix::open(repo_path).context("Failed to open repository")?;
    let workdir = repo.workdir().unwrap_or_else(|| repo.git_dir()).to_owned();

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

    let push_url = args
        .push_url
        .as_deref()
        .map(to_absolute_remote_url)
        .transpose()?;

    // Write remote config to .git/josh/remotes/<name>.josh
    write_remote_config(
        repo_path,
        &args.name,
        &remote_url,
        &filter_to_store,
        &refspec,
        forge,
        push_url.as_deref(),
        args.forge_args.gerrit_mode,
    )
    .context("Failed to write remote config file")?;

    // Set up a git remote that points to "." with a refspec to fetch filtered refs
    // Add remote pointing to current directory
    let repo_remote = to_absolute_remote_url(&workdir.display().to_string())?;
    GitCommand::new(
        repo.git_dir(),
        ["remote", "add", &args.name, &repo_remote],
        std::iter::empty::<(&str, &str)>(),
    )
    .spawn()
    .context("Failed to add git remote")?;

    // Set up namespace configuration for the remote
    let namespace = format!("josh-{}", args.name);
    let uploadpack_cmd = format!("env GIT_NAMESPACE={} git upload-pack", namespace);

    GitCommand::new(
        repo.git_dir(),
        [
            "config",
            &format!("remote.{}.uploadpack", args.name),
            &uploadpack_cmd,
        ],
        std::iter::empty::<(&str, &str)>(),
    )
    .spawn()
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
    let repo_path = normalize_repo_path(transaction.path());

    let config = read_remote_config(&repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    let filter = config.semantic_filter();
    let filter_str = josh_core::filter::spec(filter);

    println!(
        "Applying filter '{}' to remote '{}'",
        filter_str, args.remote
    );

    let default_branch = josh_cli::remote_ops::resolve_default_branch(transaction, &args.remote)?;

    josh_cli::remote_ops::apply_josh_filtering(transaction, filter, &args.remote, &default_branch)?;

    println!(
        "Applied filter '{}' to remote '{}'",
        filter_str, args.remote
    );

    Ok(())
}
