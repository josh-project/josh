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
use josh_cli::context::{CachePolicy, ExecutionContext};
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

    /// Josh store directory
    #[arg(long, global = true)]
    pub store: Option<std::path::PathBuf>,

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

/// Commands that require a git repository and transaction context.
#[derive(Debug, clap::Subcommand)]
pub enum RepoCommand {
    /// Manage stacked changes (publish, etc.)
    Changes(ChangesArgs),

    #[command(flatten)]
    Store(StoreCommand),

    #[command(flatten)]
    Local(LocalRepoCommand),
}

/// Commands backed by a worktree-centric store.
#[derive(Debug, clap::Subcommand)]
pub enum StoreCommand {
    /// Fetch upstream history into the store and refresh its worktrees
    Fetch(FetchArgs),
    /// Relay refs from a worktree projection to an upstream
    Push(PushArgs),
    /// Fetch from the store and integrate the current branch
    Pull(PullArgs),
    /// Manage store upstreams
    Remote(RemoteArgs),
    /// Manage filtered worktrees
    Worktree(WorktreeArgs),
    /// Manage links: store-wide named views bound to external remotes
    Link(LinkArgs),
    /// Manage the distributed filter cache
    Cache(CacheArgs),
    /// Manage test-forge state (test-suite plumbing)
    #[command(hide = true)]
    Forge(josh_cli::commands::forge_cmd::ForgeArgs),
}

/// Repository commands that do not require a josh store.
#[derive(Debug, clap::Subcommand)]
pub enum LocalRepoCommand {
    /// Run workspaces in containers
    Compose(ComposeArgs),
}

/// Commands that don't require a git repository
#[derive(Debug, clap::Subcommand)]
pub enum StandaloneCommand {
    /// Clone a repository into a filtered worktree
    Clone(CloneArgs),

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
pub struct InitArgs {}

#[derive(Debug, clap::Parser)]
pub struct CloneArgs {
    /// Remote repository URL
    pub url: String,

    /// Checkout directory; defaults to the repository name
    pub directory: Option<std::path::PathBuf>,

    /// Projection filter relative to the remote repository
    #[arg(long, add = josh_cli::completion::filter_syntax())]
    pub filter: Option<String>,

    /// Store mount path; defaults to the remote repository identity
    #[arg(long)]
    pub mount: Option<std::path::PathBuf>,

    /// Branch or ref to check out
    #[arg(
        short = 'b',
        long = "branch",
        default_value = "HEAD",
        add = josh_cli::completion::git_ref()
    )]
    pub branch: String,

    /// Separate push destination, usually a fork
    #[arg(long = "push-url")]
    pub push_url: Option<String>,

    #[command(flatten)]
    pub forge_args: ForgeArgs,
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

    let cache_policy = CachePolicy::new(cli.distributed_cache);
    let result = match &cli.command {
        Command::Standalone(cmd) => run_standalone(cmd, cache_policy, cli.store.as_deref()),
        Command::Repo(cmd) => run_repo(cmd, cache_policy, cli.store.as_deref()),
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

fn run_standalone(
    cmd: &StandaloneCommand,
    cache_policy: CachePolicy,
    store_override: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    match cmd {
        StandaloneCommand::Clone(args) => run_clone(args, cache_policy, store_override),
        StandaloneCommand::Init(_) => {
            let path = josh_cli::store::Store::configured_path(store_override)?;
            let store = josh_cli::store::Store::init(&path)?;
            println!("Initialized josh store at {}", store.root.display());
            Ok(())
        }
        StandaloneCommand::Migrate(args) => {
            let worktree = josh_cli::store::migrate(&args.directory, &args.worktree, cache_policy)?;
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

    // Unix can hand the wire-protocol process directly to Git. Other
    // platforms spawn it and propagate its exit status instead.
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
fn open_store_context(
    cache_policy: CachePolicy,
    store_override: Option<&std::path::Path>,
    create: bool,
) -> anyhow::Result<(josh_cli::store::Store, ExecutionContext)> {
    let store = josh_cli::store::Store::resolve(&std::env::current_dir()?, store_override, create)?;
    let git_dir = store.git_dir();
    let context = cache_policy.open(&git_dir, &git_dir, true, false)?;
    Ok((store, context))
}

fn open_repository_context(
    cache_policy: CachePolicy,
    sled: bool,
    ephemeral: bool,
) -> anyhow::Result<ExecutionContext> {
    let paths = josh_core::git::discover_repository_paths().context("Not in a git repository")?;
    cache_policy.open(&paths.workdir_or_gitdir, &paths.common_dir, sled, ephemeral)
}

fn run_repo(
    cmd: &RepoCommand,
    cache_policy: CachePolicy,
    store_override: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    match cmd {
        RepoCommand::Store(cmd) => run_store_command(cmd, cache_policy, store_override),
        RepoCommand::Local(cmd) => run_local_repo_command(cmd, cache_policy),
        RepoCommand::Changes(args) => run_changes(args, cache_policy, store_override),
    }
}

fn run_store_command(
    cmd: &StoreCommand,
    cache_policy: CachePolicy,
    store_override: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let create = matches!(cmd, StoreCommand::Remote(_));
    let (mut store, context) = open_store_context(cache_policy, store_override, create)?;
    let transaction = context.transaction();
    match cmd {
        StoreCommand::Fetch(args) => {
            josh_cli::store::fetch(&mut store, &context, args.remote.as_deref())
        }
        StoreCommand::Push(args) => {
            josh_cli::store::push(&store, args, transaction)?;
            if args.dry_run {
                Ok(())
            } else {
                josh_cli::store::fetch_quiet(&mut store, &context, args.remote.as_deref())
            }
        }
        StoreCommand::Pull(args) => josh_cli::store::pull(&mut store, args, &context),
        StoreCommand::Remote(args) => handle_remote(args, &mut store, &context),
        StoreCommand::Worktree(args) => handle_worktree(args, &mut store, transaction),
        StoreCommand::Link(args) => josh_cli::store::handle_link(&store, args, transaction),
        StoreCommand::Cache(args) => {
            josh_cli::commands::cache::handle_cache(args, &store, transaction)
        }
        StoreCommand::Forge(args) => josh_cli::commands::forge_cmd::handle_forge(args, &store),
    }
}

fn run_local_repo_command(cmd: &LocalRepoCommand, cache_policy: CachePolicy) -> anyhow::Result<()> {
    let LocalRepoCommand::Compose(args) = cmd;
    let ephemeral = matches!(
        args.command,
        ComposeCommand::Run(_)
            | ComposeCommand::Shell(_)
            | ComposeCommand::Graph(_)
            | ComposeCommand::ListImages(_)
            | ComposeCommand::ListJobs(_)
    );
    let context = open_repository_context(cache_policy, false, ephemeral)?;
    josh_cli::commands::compose::handle_compose(args, context.transaction())
}

fn run_changes(
    args: &ChangesArgs,
    cache_policy: CachePolicy,
    store_override: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    if matches!(
        args.command,
        ChangesCommand::Publish(_) | ChangesCommand::Pull(_)
    ) {
        let (mut store, context) = open_store_context(cache_policy, store_override, false)?;
        let transaction = context.transaction();
        return match &args.command {
            ChangesCommand::Publish(publish_args) => {
                josh_cli::store::publish(&store, publish_args, transaction)?;
                if publish_args.dry_run {
                    Ok(())
                } else {
                    josh_cli::store::fetch_quiet(
                        &mut store,
                        &context,
                        publish_args.remote.as_deref(),
                    )
                }
            }
            ChangesCommand::Pull(pull_args) => {
                josh_cli::store::pull_changes(&mut store, pull_args, &context)
            }
            _ => unreachable!("store-backed changes command checked above"),
        };
    }

    let context = open_repository_context(cache_policy, true, false)?;
    let transaction = context.transaction();
    match &args.command {
        ChangesCommand::List(list_args) => {
            josh_cli::commands::changes::handle_list(list_args, transaction)
        }
        ChangesCommand::Show(show_args) => {
            josh_cli::commands::changes::handle_show(show_args, transaction)
        }
        ChangesCommand::Deps(deps_args) => {
            josh_cli::commands::changes::handle_deps(deps_args, transaction)
        }
        ChangesCommand::Comment(comment_args) => {
            josh_cli::commands::comment::handle_comment(comment_args, transaction)?;
            transaction.flush_mem_odb()?;
            if let Ok(store) = josh_cli::store::Store::discover(&std::env::current_dir()?) {
                josh_cli::store::sync_worktree_change_metadata(&store, cache_policy)?;
            }
            Ok(())
        }
        ChangesCommand::Sync(sync_args) => {
            let store = josh_cli::store::Store::discover(&std::env::current_dir()?).ok();
            josh_cli::commands::sync::handle_sync(sync_args, store.as_ref(), transaction)?;
            transaction.flush_mem_odb()?;
            if let Some(store) = store {
                josh_cli::store::sync_worktree_change_metadata(&store, cache_policy)?;
            }
            Ok(())
        }
        ChangesCommand::Publish(_) | ChangesCommand::Pull(_) => {
            unreachable!("store-backed changes commands returned above")
        }
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

fn clone_repository_name(url: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(url).with_context(|| format!("Invalid remote URL '{url}'"))?;
    let name = parsed
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
        .map(|segment| segment.strip_suffix(".git").unwrap_or(segment))
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Could not derive a repository name from '{url}'"))?;
    Ok(name.to_owned())
}

fn clone_mount(url: &str, github: bool) -> anyhow::Result<std::path::PathBuf> {
    let repository = clone_repository_name(url)?;
    let parsed = url::Url::parse(url).with_context(|| format!("Invalid remote URL '{url}'"))?;
    let Some(host) = github.then(|| parsed.host_str()).flatten() else {
        return Ok(repository.into());
    };

    let mut mount = std::path::PathBuf::from(host.to_ascii_lowercase());
    let mut segments = parsed
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let last = segments
        .last_mut()
        .ok_or_else(|| anyhow::anyhow!("Could not derive a repository path from '{url}'"))?;
    if let Some(stripped) = last.strip_suffix(".git") {
        *last = stripped.to_owned();
    }
    for segment in segments {
        mount.push(segment);
    }
    Ok(mount)
}

fn verify_existing_clone_remote(
    store: &josh_cli::store::Store,
    name: &str,
    url: &str,
    push_url: Option<&str>,
    forge_args: &ForgeArgs,
) -> anyhow::Result<()> {
    let config = store.remote_config(name)?;
    if config.url != url {
        anyhow::bail!(
            "Mount is already used by remote '{}' with URL '{}'; pass --mount to choose another path",
            name,
            config.url
        );
    }
    if let Some(push_url) = push_url
        && config.push_url.as_deref() != Some(push_url)
    {
        anyhow::bail!("Remote '{}' already has a different push URL", name);
    }
    let requested_forge = if forge_args.no_forge {
        Some(None)
    } else {
        forge_args.forge.map(Some)
    };
    if requested_forge.is_some_and(|forge| config.forge != forge) {
        anyhow::bail!(
            "Remote '{}' already has different forge configuration",
            name
        );
    }
    if forge_args
        .gerrit_mode
        .is_some_and(|mode| config.gerrit_mode != mode)
    {
        anyhow::bail!("Remote '{}' already has a different Gerrit mode", name);
    }
    Ok(())
}

fn run_clone(
    args: &CloneArgs,
    cache_policy: CachePolicy,
    store_override: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let current_dir = std::env::current_dir()?;
    let remote_url = to_absolute_remote_url(&args.url)?;
    let guessed_forge = josh_cli::forge::guess_forge(&remote_url);
    let github_mount =
        guessed_forge == Some(Forge::Github) || args.forge_args.forge == Some(Forge::Github);
    let repository = clone_repository_name(&remote_url)?;
    let mount = args
        .mount
        .clone()
        .map(Ok)
        .unwrap_or_else(|| clone_mount(&remote_url, github_mount))?;
    let worktree = args
        .directory
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from(&repository));
    let worktree = if worktree.is_absolute() {
        worktree
    } else {
        current_dir.join(worktree)
    };
    let push_url = args
        .push_url
        .as_deref()
        .map(to_absolute_remote_url)
        .transpose()?;
    let forge = if args.forge_args.no_forge {
        None
    } else {
        args.forge_args.forge.or(guessed_forge)
    };

    let mut store = josh_cli::store::Store::resolve(&current_dir, store_override, true)?;
    let git_dir = store.git_dir();
    let context = cache_policy.open(&git_dir, &git_dir, true, false)?;
    let transaction = context.transaction();

    let (remote, added) = match store.remote_name_at_mount(&mount)? {
        Some(remote) => {
            verify_existing_clone_remote(
                &store,
                &remote,
                &remote_url,
                push_url.as_deref(),
                &args.forge_args,
            )?;
            josh_cli::store::fetch_quiet(&mut store, &context, Some(&remote))?;
            (remote, false)
        }
        None => {
            let remote = josh_cli::store::add_remote(
                &mut store,
                &remote_url,
                &mount,
                forge,
                push_url.as_deref(),
                args.forge_args.gerrit_mode,
            )?;
            if let Err(error) = josh_cli::store::fetch_quiet(&mut store, &context, Some(&remote)) {
                josh_cli::store::rollback_remote(&mut store, &remote, transaction)
                    .context("Failed to roll back remote after fetch failure")?;
                return Err(error);
            }
            eprintln!("Added remote '{}' at '{}'", remote, mount.display());
            (remote, true)
        }
    };

    let mount_filter = josh_core::filter::to_filter(josh_core::filter::Op::Subdir(
        store.config.remotes[&remote].mount.clone(),
    ));
    let projection = josh_core::filter::parse(args.filter.as_deref().unwrap_or(":/"))?;
    let filter = josh_core::filter::spec(mount_filter.chain(projection));
    let branch = (args.branch != "HEAD").then_some(args.branch.as_str());
    let worktree = match josh_cli::store::add_worktree(
        &mut store,
        &worktree,
        &filter,
        branch,
        &args.branch,
        transaction,
    ) {
        Ok(worktree) => worktree,
        Err(error) => {
            if added {
                josh_cli::store::rollback_remote(&mut store, &remote, &transaction)
                    .context("Failed to roll back remote after clone failure")?;
            }
            return Err(error);
        }
    };
    println!("Cloned worktree to {}", worktree.display());
    Ok(())
}

fn handle_remote(
    args: &RemoteArgs,
    store: &mut josh_cli::store::Store,
    context: &ExecutionContext,
) -> anyhow::Result<()> {
    let transaction = context.transaction();
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
            if let Err(error) = josh_cli::store::fetch(store, context, Some(&name)) {
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
                    let url = module.serialized_url()?;
                    let forge = josh_cli::forge::guess_forge(&url);
                    let name = josh_cli::store::add_remote(store, &url, mount, forge, None, None)?;
                    if let Err(error) = josh_cli::store::fetch(store, context, Some(&name)) {
                        josh_cli::store::rollback_remote(store, &name, transaction)
                            .context("Failed to roll back submodule remote after fetch failure")?;
                        return Err(error);
                    }
                    eprintln!(
                        "Added submodule remote '{}' at '{}': {}",
                        name,
                        mount.display(),
                        url
                    );
                }
            }
            Ok(())
        }
    }
}

fn absolute_worktree_path(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(josh_core::normalize_path(&path))
}

fn handle_worktree(
    args: &WorktreeArgs,
    store: &mut josh_cli::store::Store,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        WorktreeCommand::Add(args) => {
            let path = absolute_worktree_path(&args.path)?;
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
            let path = absolute_worktree_path(&args.path)?;
            josh_cli::store::remove_worktree(store, &path, args.force, transaction)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clone_mount, clone_repository_name};
    use std::path::Path;

    #[test]
    fn github_clone_mount_includes_host_and_owner() {
        assert_eq!(
            clone_mount("https://github.com/rust-lang/rust.git", true).unwrap(),
            Path::new("github.com/rust-lang/rust")
        );
        assert_eq!(
            clone_mount(
                "ssh://git@github.company.example/platform/project.git",
                true,
            )
            .unwrap(),
            Path::new("github.company.example/platform/project")
        );
    }

    #[test]
    fn ordinary_clone_mount_uses_repository_name() {
        assert_eq!(
            clone_mount("https://git.example.com/team/project.git", false).unwrap(),
            Path::new("project")
        );
        assert_eq!(
            clone_repository_name("file:///tmp/project.git").unwrap(),
            "project"
        );
    }
}
