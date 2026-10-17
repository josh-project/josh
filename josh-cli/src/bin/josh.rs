use anyhow::Context;
use clap::{CommandFactory, Parser};

use josh_cli::commands::auth::AuthArgs;
use josh_cli::commands::cache::CacheArgs;
use josh_cli::commands::changes::{DepsArgs, ListArgs, ShowArgs};
use josh_cli::commands::comment::CommentArgs;
use josh_cli::commands::compose::{ComposeArgs, ComposeCommand};
use josh_cli::commands::fetch::FetchArgs;
use josh_cli::commands::link::LinkArgs;
use josh_cli::commands::pull::PullArgs;
use josh_cli::commands::push::{PublishArgs, PushArgs};
use josh_cli::commands::sync::SyncArgs;
use josh_cli::forge::{Forge, GerritMode};

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
    /// Fetch upstream history into the store and refresh its worktrees
    Fetch(FetchArgs),

    /// Relay refs from a worktree projection to an upstream
    Push(PushArgs),

    /// Fetch from the store and integrate the current branch
    Pull(PullArgs),

    /// Manage stacked changes (publish, etc.)
    Changes(ChangesArgs),

    /// Manage store upstreams
    Remote(RemoteArgs),

    /// Manage filtered worktrees
    Worktree(WorktreeArgs),

    /// Manage links: named views of the repository bound to external remotes
    Link(LinkArgs),

    /// Manage the distributed filter cache
    Cache(CacheArgs),

    /// Run workspaces in containers
    Compose(ComposeArgs),

    /// Manage test-forge state (test-suite plumbing)
    #[command(hide = true)]
    Forge(josh_cli::commands::forge_cmd::ForgeArgs),
}

/// Commands that don't require a git repository
#[derive(Debug, clap::Subcommand)]
pub enum StandaloneCommand {
    /// Create a checkout-less josh store
    Init(InitArgs),

    /// Convert an old filter-bearing clone into a store
    Migrate(MigrateArgs),

    /// Manage forge authentication
    Auth(AuthArgs),

    /// Serve a worktree's local Git remote
    #[command(hide = true)]
    ServeWorktree(ServeWorktreeArgs),
}

#[derive(Debug, clap::Parser)]
pub struct InitArgs {
    /// Store directory
    #[arg(default_value = ".")]
    pub directory: std::path::PathBuf,
}
#[derive(Debug, clap::Parser)]
pub struct MigrateArgs {
    /// Repository to migrate
    #[arg(default_value = ".")]
    pub directory: std::path::PathBuf,

    /// Name of the linked worktree created inside the store
    #[arg(long, default_value = "worktree")]
    pub worktree: String,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum GitService {
    UploadPack,
    ReceivePack,
}

#[derive(Debug, clap::Parser)]
pub struct ServeWorktreeArgs {
    /// Store namespace backing this virtual remote
    #[arg(long)]
    pub namespace: String,

    /// Git wire-protocol service
    #[arg(value_enum)]
    pub service: GitService,

    /// Store repository path appended by Git
    pub repository: std::path::PathBuf,
}

#[derive(Debug, clap::Parser)]
pub struct WorktreeArgs {
    /// Worktree subcommand
    #[command(subcommand)]
    pub command: WorktreeCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum WorktreeCommand {
    /// Create a linked repository at a projection
    Add(WorktreeAddArgs),
    /// List the store's registered worktrees
    List,
    /// Remove a registered worktree
    Remove(WorktreeRemoveArgs),
}

#[derive(Debug, clap::Parser)]
pub struct WorktreeAddArgs {
    /// Checkout path
    pub path: std::path::PathBuf,

    /// Projection filter; omit for the full repository
    #[arg(conflicts_with = "submodules", add = josh_cli::completion::filter_syntax())]
    pub filter: Option<String>,

    /// Inline configured submodules and create their export links
    #[arg(long)]
    pub submodules: bool,

    /// Superproject remote used by --submodules
    #[arg(long, requires = "submodules", add = josh_cli::completion::remote())]
    pub remote: Option<String>,

    /// Local branch to create
    #[arg(short = 'b', long = "branch")]
    pub branch: Option<String>,

    /// Store ref to check out
    #[arg(long = "ref", default_value = "HEAD", add = josh_cli::completion::git_ref())]
    pub rref: String,
}

#[derive(Debug, clap::Parser)]
pub struct WorktreeRemoveArgs {
    /// Registered worktree path
    pub path: std::path::PathBuf,

    /// Remove a worktree with uncommitted changes
    #[arg(long)]
    pub force: bool,
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
    /// Add a remote mounted at a path
    Add(RemoteAddArgs),
}

#[derive(Debug, Clone, clap::Parser)]
pub struct RemoteAddArgs {
    /// Non-root relative mount path; also determines the local remote identity
    #[arg()]
    pub path: std::path::PathBuf,

    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Also discover and mount the configured submodule remotes
    #[arg(long)]
    pub submodules: bool,

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

fn main() -> std::process::ExitCode {
    josh_cli::completion::complete(Cli::command);
    let _flush_guard = josh_core::memodb::FlushGuard::new();
    env_logger::init();
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Standalone(cmd) => run_standalone(cmd, cli.distributed_cache),
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

fn run_standalone(cmd: &StandaloneCommand, distributed_cache: bool) -> anyhow::Result<()> {
    match cmd {
        StandaloneCommand::Init(args) => {
            let store = josh_cli::store::Store::init(&args.directory)?;
            println!("Initialized josh store at {}", store.root.display());
            Ok(())
        }
        StandaloneCommand::Migrate(args) => {
            let worktree =
                josh_cli::store::migrate(&args.directory, &args.worktree, distributed_cache)?;
            println!("Migrated worktree to {}", worktree.display());
            Ok(())
        }
        StandaloneCommand::ServeWorktree(args) => serve_worktree(args),
        StandaloneCommand::Auth(args) => josh_cli::commands::auth::handle_auth(args),
    }
}

fn serve_worktree(args: &ServeWorktreeArgs) -> anyhow::Result<()> {
    let repository = josh_cli::store::validate_worktree_service(&args.repository, &args.namespace)?;

    let service = match args.service {
        GitService::UploadPack => "upload-pack",
        GitService::ReceivePack => "receive-pack",
    };
    let mut command = std::process::Command::new("git");
    command
        .arg(service)
        .arg(&repository)
        .env("GIT_NAMESPACE", &args.namespace);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        return Err(command.exec()).context("Failed to execute Git service");
    }

    #[cfg(not(unix))]
    {
        let status = command.status().context("Failed to execute Git service")?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }
}
fn run_repo(cmd: &RepoCommand, distributed_cache: bool) -> anyhow::Result<()> {
    let store_command = matches!(
        cmd,
        RepoCommand::Fetch(_)
            | RepoCommand::Push(_)
            | RepoCommand::Pull(_)
            | RepoCommand::Remote(_)
            | RepoCommand::Worktree(_)
            | RepoCommand::Changes(ChangesArgs {
                command: ChangesCommand::Publish(_) | ChangesCommand::Pull(_),
            })
            | RepoCommand::Cache(_)
            | RepoCommand::Forge(_)
    );
    let mut store = if store_command {
        Some(josh_cli::store::Store::discover(&std::env::current_dir()?)?)
    } else {
        None
    };
    let (repo_path, git_common_dir) = if let Some(store) = &store {
        (store.git_dir(), store.git_dir())
    } else {
        let paths =
            josh_core::git::discover_repository_paths().context("Not in a git repository")?;
        (paths.workdir_or_gitdir, paths.common_dir)
    };

    let is_compose = matches!(cmd, RepoCommand::Compose(_));
    let ephemeral_compose = match cmd {
        RepoCommand::Compose(args) => match &args.command {
            ComposeCommand::Run(_)
            | ComposeCommand::Shell(_)
            | ComposeCommand::Graph(_)
            | ComposeCommand::ListImages(_)
            | ComposeCommand::ListJobs(_) => true,
            ComposeCommand::Clean(_) | ComposeCommand::Pull(_) | ComposeCommand::Push(_) => false,
        },
        _ => false,
    };

    let mut cache_stack = josh_core::cache::CacheStack::new();
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

    let mut ctx = josh_core::cache::TransactionContext::new(&repo_path, cache);
    if ephemeral_compose {
        ctx = ctx.ephemeral();
    } else {
        ctx = ctx.with_mem_odb_limit(josh_cli::MAX_MEM_PACK_SIZE);
    }
    let transaction = ctx.open().context("Failed TransactionContext::open")?;

    match cmd {
        RepoCommand::Fetch(args) => josh_cli::store::fetch(
            store.as_ref().expect("store command has store"),
            &transaction,
            args.remote.as_deref(),
            distributed_cache,
        ),
        RepoCommand::Push(args) => {
            let store = store.as_ref().expect("store command has store");
            josh_cli::store::push(store, args, &transaction)?;
            if args.dry_run {
                Ok(())
            } else {
                josh_cli::store::fetch_quiet(
                    store,
                    &transaction,
                    args.remote.as_deref(),
                    distributed_cache,
                )
            }
        }
        RepoCommand::Pull(args) => josh_cli::store::pull(
            store.as_ref().expect("store command has store"),
            args,
            &transaction,
            distributed_cache,
        ),
        RepoCommand::Changes(args) => match &args.command {
            ChangesCommand::Publish(publish_args) => {
                let store = store.as_ref().expect("store command has store");
                josh_cli::store::publish(store, publish_args, &transaction)?;
                if publish_args.dry_run {
                    Ok(())
                } else {
                    josh_cli::store::fetch_quiet(
                        store,
                        &transaction,
                        publish_args.remote.as_deref(),
                        distributed_cache,
                    )
                }
            }
            ChangesCommand::List(list_args) => {
                josh_cli::commands::changes::handle_list(list_args, &transaction)
            }
            ChangesCommand::Pull(pull_args) => josh_cli::store::pull_changes(
                store.as_ref().expect("store command has store"),
                pull_args,
                &transaction,
                distributed_cache,
            ),
            ChangesCommand::Show(show_args) => {
                josh_cli::commands::changes::handle_show(show_args, &transaction)
            }
            ChangesCommand::Deps(deps_args) => {
                josh_cli::commands::changes::handle_deps(deps_args, &transaction)
            }
            ChangesCommand::Comment(comment_args) => {
                josh_cli::commands::comment::handle_comment(comment_args, &transaction)?;
                transaction.flush_mem_odb()?;
                if let Ok(store) = josh_cli::store::Store::discover(&std::env::current_dir()?) {
                    josh_cli::store::sync_worktree_change_metadata(&store, distributed_cache)?;
                }
                Ok(())
            }
            ChangesCommand::Sync(sync_args) => {
                josh_cli::commands::sync::handle_sync(sync_args, &transaction)?;
                transaction.flush_mem_odb()?;
                if let Ok(store) = josh_cli::store::Store::discover(&std::env::current_dir()?) {
                    josh_cli::store::sync_worktree_change_metadata(&store, distributed_cache)?;
                }
                Ok(())
            }
        },
        RepoCommand::Remote(args) => handle_remote(
            args,
            store.as_mut().expect("store command has store"),
            &transaction,
            distributed_cache,
        ),
        RepoCommand::Worktree(args) => handle_worktree(
            args,
            store.as_mut().expect("store command has store"),
            &transaction,
        ),
        RepoCommand::Link(args) => josh_cli::commands::link::handle_link(args, &transaction),
        RepoCommand::Compose(args) => {
            josh_cli::commands::compose::handle_compose(args, &transaction)
        }
        RepoCommand::Cache(args) => josh_cli::commands::cache::handle_cache(args, &transaction),
        RepoCommand::Forge(args) => josh_cli::commands::forge_cmd::handle_forge(args, &transaction),
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

fn handle_remote(
    args: &RemoteArgs,
    store: &mut josh_cli::store::Store,
    transaction: &josh_core::cache::Transaction,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    match &args.command {
        RemoteCommand::Add(add_args) => {
            let remote_url = to_absolute_remote_url(&add_args.url)?;
            if add_args.submodules {
                josh_core::filter::check_experimental_features_enabled(
                    "josh remote add --submodules",
                )?;
            }

            let push_url = add_args
                .push_url
                .as_deref()
                .map(to_absolute_remote_url)
                .transpose()?;
            let forge = if add_args.forge_args.no_forge {
                None
            } else {
                add_args
                    .forge_args
                    .forge
                    .or_else(|| josh_cli::forge::guess_forge(&remote_url))
            };
            let name = josh_cli::store::add_remote(
                store,
                &remote_url,
                &add_args.path,
                forge,
                push_url.as_deref(),
                add_args.forge_args.gerrit_mode,
            )?;
            if let Err(error) =
                josh_cli::store::fetch(store, transaction, Some(&name), distributed_cache)
            {
                josh_cli::store::rollback_remote(store, &name, transaction)
                    .context("Failed to roll back remote after fetch failure")?;
                return Err(error);
            }
            eprintln!("Added remote '{}' at '{}'", name, add_args.path.display());
            if add_args.submodules {
                let modules =
                    josh_cli::store::discover_submodule_remotes(store, &name, transaction)?;
                for module in modules {
                    let mount = std::path::Path::new(&module.name);
                    let forge = josh_cli::forge::guess_forge(&module.url);
                    let name =
                        josh_cli::store::add_remote(store, &module.url, mount, forge, None, None)?;
                    if let Err(error) =
                        josh_cli::store::fetch(store, transaction, Some(&name), distributed_cache)
                    {
                        josh_cli::store::rollback_remote(store, &name, transaction)
                            .context("Failed to roll back submodule remote after fetch failure")?;
                        return Err(error);
                    }
                    eprintln!(
                        "Added submodule remote '{}' at '{}': {}",
                        name,
                        mount.display(),
                        module.url
                    );
                }
            }
            Ok(())
        }
    }
}

fn handle_worktree(
    args: &WorktreeArgs,
    store: &mut josh_cli::store::Store,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        WorktreeCommand::Add(args) => {
            let path = if args.path.is_absolute() {
                args.path.clone()
            } else {
                std::env::current_dir()?.join(&args.path)
            };
            let filter = if args.submodules {
                josh_core::filter::check_experimental_features_enabled(
                    "josh worktree add --submodules",
                )?;
                let remote = match args.remote.as_deref() {
                    Some(remote) => remote,
                    None if store.config.remotes.len() == 1 => {
                        store.config.remotes.keys().next().expect("length checked")
                    }
                    None => anyhow::bail!(
                        "Multiple remotes are configured; --submodules requires --remote"
                    ),
                };
                josh_cli::store::derive_submodule_filter(store, remote, transaction)?
            } else {
                args.filter.clone().unwrap_or_else(|| ":/".to_owned())
            };
            let path = josh_cli::store::add_worktree(
                store,
                &path,
                &filter,
                args.branch.as_deref(),
                &args.rref,
                transaction,
            )?;
            println!("Added worktree at {}", path.display());
            Ok(())
        }
        WorktreeCommand::List => {
            josh_cli::store::list_worktrees(store);
            Ok(())
        }
        WorktreeCommand::Remove(args) => {
            josh_cli::store::remove_worktree(store, &args.path, args.force, transaction)
        }
    }
}
