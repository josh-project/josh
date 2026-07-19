use anyhow::Context;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use josh_cli::commands::agent::AgentArgs;
use josh_cli::commands::auth::AuthArgs;
use josh_cli::commands::cache::CacheArgs;
use josh_cli::commands::capabilities::CapabilitiesArgs;
use josh_cli::commands::changes::ListArgs;
use josh_cli::commands::comment::CommentArgs;
use josh_cli::commands::link::LinkArgs;
use josh_cli::commands::push::{PublishArgs, PushArgs};
use josh_cli::commands::run::ComposeArgs;
use josh_cli::commands::status::StatusArgs;
use josh_cli::commands::sync::SyncArgs;
use josh_cli::commands::workspace::WorkspaceArgs;
use josh_cli::config::{
    RemoteConfig, list_remote_names, read_remote_config, remove_remote_config, write_remote_config,
};
use josh_cli::forge::Forge;
use josh_cli::output::{ColorChoice, OutputFormat, OutputOptions};
use josh_cli::{cli_eprintln as eprintln, cli_println as println};
use josh_core::git::{CommandColor, normalize_repo_path, spawn_git_command};

#[derive(Debug, clap::Parser)]
#[command(
    name = "josh",
    version = josh_core::VERSION,
    about = "Josh: Git projections & sync tooling",
    long_about = None,
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Subcommand to run
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Args)]
pub struct GlobalArgs {
    /// Output format; JSON schemas are versioned and stable
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "human",
        env = "JOSH_OUTPUT"
    )]
    pub output: OutputFormat,

    /// Pretty-print --output json instead of emitting one compact line
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Suppress diagnostics and omit messages from machine output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Control color in Josh and child Git processes
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "auto",
        env = "JOSH_COLOR"
    )]
    pub color: ColorChoice,

    /// Disable progress displays and capture child process output
    #[arg(long, global = true)]
    pub no_progress: bool,

    /// Disable prompts, browser launches, and credential input
    #[arg(long, global = true, env = "JOSH_NON_INTERACTIVE")]
    pub non_interactive: bool,
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

    /// Manage stacked changes (publish, etc.)
    Changes(ChangesArgs),

    /// Manage projection-aware remotes
    Remote(RemoteArgs),

    /// Apply filtering to existing refs (like `josh fetch` but without fetching)
    Filter(FilterArgs),

    /// Manage josh links (like `josh remote` but for links)
    Link(LinkArgs),

    /// Manage the distributed filter cache
    Cache(CacheArgs),

    /// Run workspaces in containers
    Compose(ComposeArgs),

    /// Create, inspect, validate, and check out projection workspaces
    Workspace(WorkspaceArgs),

    /// Show repository, working tree, and Josh remote status
    Status(StatusArgs),
}

/// Commands that don't require a git repository
#[derive(Debug, clap::Subcommand)]
pub enum StandaloneCommand {
    /// Install or print resources for coding agents
    Agent(AgentArgs),

    /// Manage forge authentication
    Auth(AuthArgs),

    /// Describe machine-readable CLI and automation capabilities
    Capabilities(CapabilitiesArgs),

    /// Generate a shell completion script
    Completions(CompletionsArgs),
}

#[derive(Debug, clap::Parser)]
pub struct CompletionsArgs {
    /// Shell for which to generate completions
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
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

    /// Branch to pull; HEAD uses the configured upstream
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

    /// Branch to fetch; HEAD fetches the configured branch set
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,
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
    /// List local changes that would be published (read-only)
    List(ListArgs),
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
    /// List configured Josh remotes
    List(RemoteListArgs),
    /// Show one configured Josh remote
    Show(RemoteShowArgs),
    /// Remove a Josh remote and its private refs
    Remove(RemoteRemoveArgs),
    /// Change the projection filter for a Josh remote
    SetFilter(RemoteSetFilterArgs),
}

#[derive(Debug, clap::Parser)]
pub struct RemoteListArgs {}

#[derive(Debug, clap::Parser)]
pub struct RemoteShowArgs {
    /// Remote name
    pub name: String,
}

#[derive(Debug, clap::Parser)]
pub struct RemoteSetFilterArgs {
    /// Remote name
    pub name: String,

    /// New reversible Josh filter
    pub filter: String,

    /// Re-filter already fetched refs immediately
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, clap::Parser)]
pub struct RemoteRemoveArgs {
    /// Remote name
    pub name: String,

    /// Show what would be removed without changing the repository
    #[arg(long)]
    pub dry_run: bool,
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
#[command(
    subcommand_precedence_over_arg = true,
    args_conflicts_with_subcommands = true
)]
pub struct FilterArgs {
    /// Remote name to apply filtering to
    pub remote: Option<String>,

    #[command(subcommand)]
    pub command: Option<FilterCommand>,
}

#[derive(Debug, clap::Subcommand)]
pub enum FilterCommand {
    /// Validate that a filter parses and can be reversed
    Validate(FilterInspectArgs),
    /// Show a filter's canonical form, identity, and inverse
    Explain(FilterInspectArgs),
}

#[derive(Debug, clap::Parser)]
pub struct FilterInspectArgs {
    /// Josh filter expression
    pub filter: String,
}

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    let requested_format = josh_cli::output::detect_format(&raw_args);
    let requested_pretty = josh_cli::output::detect_pretty(&raw_args);
    let cli = match Cli::try_parse_from(&raw_args) {
        Ok(cli) => cli,
        Err(error) => {
            let success = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let exit_code = error.exit_code();
            josh_cli::output::render_clap(
                &error.to_string(),
                requested_format,
                requested_pretty,
                success,
                exit_code,
            );
            std::process::exit(if success { 0 } else { exit_code });
        }
    };

    env_logger::init();
    let options = OutputOptions {
        format: cli.global.output,
        color: cli.global.color,
        pretty: cli.global.pretty,
        quiet: cli.global.quiet,
        no_progress: cli.global.no_progress,
        non_interactive: cli.global.non_interactive,
    };
    josh_cli::output::init(options.clone(), cli.command.path());
    josh_core::git::configure_command_output(
        options.format != OutputFormat::Human,
        options.quiet,
        options.no_progress,
        options.non_interactive || options.format != OutputFormat::Human,
        match options.color {
            ColorChoice::Auto => CommandColor::Auto,
            ColorChoice::Always => CommandColor::Always,
            ColorChoice::Never => CommandColor::Never,
        },
    );

    let result = match &cli.command {
        Command::Standalone(cmd) => run_standalone(cmd),
        Command::Repo(cmd) => run_repo(cmd),
    };

    match result {
        Ok(()) => josh_cli::output::finish_success(),
        Err(error) => {
            josh_cli::output::finish_error(&error);
            std::process::exit(1);
        }
    }
}

impl Command {
    fn path(&self) -> &'static str {
        match self {
            Command::Repo(command) => command.path(),
            Command::Standalone(command) => command.path(),
        }
    }
}

impl RepoCommand {
    fn path(&self) -> &'static str {
        match self {
            RepoCommand::Clone(_) => "clone",
            RepoCommand::Fetch(_) => "fetch",
            RepoCommand::Pull(_) => "pull",
            RepoCommand::Push(_) => "push",
            RepoCommand::Changes(args) => match &args.command {
                ChangesCommand::Publish(_) => "changes.publish",
                ChangesCommand::List(_) => "changes.list",
                ChangesCommand::Comment(_) => "changes.comment",
                ChangesCommand::Sync(_) => "changes.sync",
            },
            RepoCommand::Remote(args) => match &args.command {
                RemoteCommand::Add(_) => "remote.add",
                RemoteCommand::List(_) => "remote.list",
                RemoteCommand::Show(_) => "remote.show",
                RemoteCommand::Remove(_) => "remote.remove",
                RemoteCommand::SetFilter(_) => "remote.set-filter",
            },
            RepoCommand::Filter(args) => match &args.command {
                Some(FilterCommand::Validate(_)) => "filter.validate",
                Some(FilterCommand::Explain(_)) => "filter.explain",
                None => "filter",
            },
            RepoCommand::Link(args) => match &args.command {
                josh_cli::commands::link::LinkCommand::Add(_) => "link.add",
                josh_cli::commands::link::LinkCommand::Fetch(_) => "link.fetch",
                josh_cli::commands::link::LinkCommand::Update(_) => "link.update",
                josh_cli::commands::link::LinkCommand::Push(_) => "link.push",
            },
            RepoCommand::Cache(args) => match &args.command {
                josh_cli::commands::cache::CacheCommand::Build(_) => "cache.build",
                josh_cli::commands::cache::CacheCommand::Push(_) => "cache.push",
                josh_cli::commands::cache::CacheCommand::Fetch(_) => "cache.fetch",
            },
            RepoCommand::Compose(args) => match &args.command {
                josh_cli::commands::run::ComposeCommand::Run(_) => "compose.run",
                josh_cli::commands::run::ComposeCommand::ListImages(_) => "compose.list-images",
                josh_cli::commands::run::ComposeCommand::ListJobs(_) => "compose.list-jobs",
            },
            RepoCommand::Workspace(args) => match &args.command {
                josh_cli::commands::workspace::WorkspaceCommand::Create(_) => "workspace.create",
                josh_cli::commands::workspace::WorkspaceCommand::List(_) => "workspace.list",
                josh_cli::commands::workspace::WorkspaceCommand::Show(_) => "workspace.show",
                josh_cli::commands::workspace::WorkspaceCommand::Validate(_) => {
                    "workspace.validate"
                }
                josh_cli::commands::workspace::WorkspaceCommand::Checkout(_) => {
                    "workspace.checkout"
                }
            },
            RepoCommand::Status(_) => "status",
        }
    }
}

impl StandaloneCommand {
    fn path(&self) -> &'static str {
        match self {
            StandaloneCommand::Agent(args) => match &args.command {
                josh_cli::commands::agent::AgentCommand::Skill(args) => match &args.command {
                    josh_cli::commands::agent::SkillCommand::Print(_) => "agent.skill.print",
                    josh_cli::commands::agent::SkillCommand::Install(_) => "agent.skill.install",
                },
            },
            StandaloneCommand::Auth(args) => match &args.command {
                josh_cli::commands::auth::AuthCommand::Login(_) => "auth.login",
                josh_cli::commands::auth::AuthCommand::Logout(_) => "auth.logout",
                josh_cli::commands::auth::AuthCommand::Debug(_) => "auth.debug",
            },
            StandaloneCommand::Capabilities(_) => "capabilities",
            StandaloneCommand::Completions(_) => "completions",
        }
    }
}

fn run_standalone(cmd: &StandaloneCommand) -> anyhow::Result<()> {
    match cmd {
        StandaloneCommand::Agent(args) => josh_cli::commands::agent::handle_agent(args),
        StandaloneCommand::Auth(args) => josh_cli::commands::auth::handle_auth(args),
        StandaloneCommand::Capabilities(args) => {
            josh_cli::commands::capabilities::handle_capabilities(args)
        }
        StandaloneCommand::Completions(args) => handle_completions(args),
    }
}

fn handle_completions(args: &CompletionsArgs) -> anyhow::Result<()> {
    let mut command = Cli::command();
    let mut script = Vec::new();
    clap_complete::generate(args.shell, &mut command, "josh", &mut script);
    let script = String::from_utf8(script).context("Completion script was not valid UTF-8")?;
    josh_cli::output::set_data_value(serde_json::json!({
        "shell": args.shell.to_string(),
        "script": script,
    }));
    if !josh_cli::output::is_machine()
        && let Err(error) = josh_cli::output::raw_stdout(&script)
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(error).context("Failed to write completion script");
    }
    Ok(())
}

fn run_repo(cmd: &RepoCommand) -> anyhow::Result<()> {
    if let RepoCommand::Status(args) = cmd {
        let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
        return josh_cli::commands::status::handle_status(args, &repo);
    }
    if let RepoCommand::Filter(FilterArgs {
        command: Some(command),
        ..
    }) = cmd
    {
        return handle_filter_inspect(command);
    }

    // For clone, do the initial repo setup before creating transaction
    let repo_path = if let RepoCommand::Clone(args) = cmd {
        // For clone, we're not in a git repo initially, so clone first and use that path
        clone_repo(args)?
    } else {
        // For other commands, we need to be in a git repo
        let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
        normalize_repo_path(repo.path())
    };

    // The josh cache lives in the repository's common git directory so it is
    // shared across linked worktrees. Reconstructing `<workdir>/.git` would be
    // wrong from a worktree, where the gitdir is shared but located elsewhere.
    let git_common_dir = git2::Repository::open(&repo_path)
        .context("Failed to open repository")?
        .commondir()
        .to_path_buf();

    josh_core::cache::sled_load(&git_common_dir).context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default())
            .with_backend(
                josh_core::cache::DistributedCacheBackend::new(&git_common_dir)
                    .context("Failed to create DistributedCacheBackend")?,
            ),
    );

    let mut ctx = josh_core::cache::TransactionContext::new(&repo_path, cache.clone());

    // For compose, we don't need to flush the objects to disk;
    // everything else gets mem odb setup with an upper flush limit
    if matches!(cmd, RepoCommand::Compose(_)) {
        ctx = ctx.ephemeral();
    } else {
        ctx = ctx.with_mem_odb_limit(josh_cli::MAX_MEM_PACK_SIZE)
    }

    let transaction = ctx.open().context("Failed TransactionContext::open")?;

    match cmd {
        RepoCommand::Clone(args) => handle_clone(args, &transaction),
        RepoCommand::Fetch(args) => {
            handle_fetch(args, &transaction)?;
            josh_cli::output::set_data_value(serde_json::json!({
                "remote": args.remote,
                "branch": args.rref,
            }));
            Ok(())
        }
        RepoCommand::Pull(args) => handle_pull(args, &transaction),
        RepoCommand::Push(args) => josh_cli::commands::push::handle_push(args, &transaction),
        RepoCommand::Changes(args) => match &args.command {
            ChangesCommand::Publish(publish_args) => {
                josh_cli::commands::push::handle_publish(publish_args, &transaction)?;
                let remote = publish_args.remote.as_deref().unwrap_or("origin");
                handle_fetch(
                    &FetchArgs {
                        remote: remote.to_string(),
                        rref: "HEAD".to_string(),
                    },
                    &transaction,
                )
            }
            ChangesCommand::List(list_args) => {
                josh_cli::commands::changes::handle_list(list_args, &transaction)
            }
            ChangesCommand::Comment(comment_args) => {
                josh_cli::commands::comment::handle_comment(comment_args, &transaction)
            }
            ChangesCommand::Sync(sync_args) => {
                josh_cli::commands::sync::handle_sync(sync_args, &transaction)?;
                josh_cli::output::set_data_value(serde_json::json!({
                    "remote": sync_args.remote.as_deref().unwrap_or("origin"),
                    "clean": sync_args.clean,
                    "local": sync_args.local,
                    "push": sync_args.push,
                    "completed": true,
                }));
                Ok(())
            }
        },
        RepoCommand::Remote(args) => handle_remote(args, &transaction),
        RepoCommand::Filter(args) => handle_filter(args, &transaction),
        RepoCommand::Link(args) => josh_cli::commands::link::handle_link(args, &transaction),
        RepoCommand::Compose(args) => josh_cli::commands::run::handle_compose(args, &transaction),
        RepoCommand::Cache(args) => josh_cli::commands::cache::handle_cache(args, &transaction),
        RepoCommand::Workspace(args) => {
            josh_cli::commands::workspace::handle_workspace(args, &transaction)
        }
        RepoCommand::Status(args) => {
            josh_cli::commands::status::handle_status(args, transaction.repo())
        }
    }
}

fn to_absolute_remote_url(url: &str) -> anyhow::Result<String> {
    let scp_like = url
        .split_once(':')
        .is_some_and(|(host, path)| host.contains('@') && !path.is_empty());
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ssh://")
        || url.starts_with("git://")
        || url.starts_with("file://")
        || scp_like
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

    if output_dir.exists()
        && std::fs::read_dir(&output_dir)?
            .next()
            .transpose()?
            .is_some()
    {
        return Err(anyhow::anyhow!(
            "Clone destination '{}' already exists and is not empty",
            output_dir.display()
        ));
    }
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

    let output_dir = normalize_repo_path(repo.path());
    let output_dir = output_dir.display().to_string();

    let output_dir = if let Ok(testtmp) = std::env::var("TESTTMP") {
        output_dir.replace(&testtmp, "${TESTTMP}")
    } else {
        output_dir.to_string()
    };

    println!("Cloned repository to: {}", output_dir);
    josh_cli::output::set_data_value(serde_json::json!({
        "path": output_dir,
        "remote": "origin",
        "url": args.url,
        "filter": args.filter,
        "branch": default_branch,
    }));
    Ok(())
}

fn handle_pull(args: &PullArgs, transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
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
    if args.rref != "HEAD" {
        git_args.push(&args.rref);
    }

    transaction
        .spawn_git(&git_args, &[])
        .context("git pull failed")?;

    eprintln!("Pulled from remote: {}", args.remote);
    josh_cli::output::set_data_value(serde_json::json!({
        "remote": args.remote,
        "branch": args.rref,
        "rebase": args.rebase,
        "autostash": args.autostash,
    }));

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

    // Fetch all configured branches by default, or only the explicitly requested branch.
    let requested_refspec = if args.rref == "HEAD" {
        ref_spec
    } else {
        let branch = args.rref.strip_prefix("refs/heads/").unwrap_or(&args.rref);
        let source = format!("refs/heads/{branch}");
        if !git2::Reference::is_valid_name(&source) {
            return Err(anyhow::anyhow!(
                "Invalid branch passed to --ref: '{}'",
                args.rref
            ));
        }
        format!("+{source}:refs/josh/remotes/{}/{branch}", args.remote)
    };
    transaction
        .spawn_git(&["fetch", &url, &requested_refspec], &[])
        .context("git fetch to josh/remotes failed")?;

    // Warm the local cache from the remote before filtering
    let filter = filter_with_meta.peel();
    if let Err(e) = josh_cli::commands::cache::fetch_remote_cache(transaction, &url, filter) {
        eprintln!("Warning: could not fetch remote cache: {e}");
    }

    // Set up remote HEAD reference using git ls-remote
    // This is the proper way to get the default branch from the remote
    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);

    // Use git ls-remote --symref to get the default branch
    // Parse the output to get the default branch name
    // Output format: "ref: refs/heads/main\t<commit-hash>"
    let mut command = std::process::Command::new("git");
    command
        .args(["ls-remote", "--symref", &url, "HEAD"])
        .current_dir(normalize_repo_path(repo.path()));
    if josh_cli::output::is_non_interactive() {
        command.env("GIT_TERMINAL_PROMPT", "0");
    }
    let output = command.output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to determine default branch: git ls-remote --symref failed for '{}'",
            args.remote
        ));
    }

    let ls_output = String::from_utf8(output.stdout)?;
    let (default_branch, default_branch_ref) = try_parse_symref(&args.remote, &ls_output)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not determine default branch from remote '{}': \
                 no symref for HEAD in ls-remote output",
                args.remote
            )
        })?;

    repo.reference_symbolic(&head_ref, &default_branch_ref, true, "josh remote HEAD")?;
    repo.reference_symbolic(
        &format!("refs/namespaces/josh-{}/{}", args.remote, "HEAD"),
        &format!("refs/heads/{}", default_branch),
        true,
        "josh remote HEAD",
    )?;

    josh_cli::remote_ops::apply_josh_filtering(transaction, filter, &args.remote, &default_branch)?;

    // Note: fetch doesn't checkout, it just updates the refs
    eprintln!("Fetched from remote: {}", args.remote);

    Ok(())
}

fn handle_remote(
    args: &RemoteArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());
    match &args.command {
        RemoteCommand::Add(add_args) => {
            handle_remote_add_repo(add_args, &repo_path)?;
            let config = read_remote_config(&repo_path, &add_args.name)?;
            josh_cli::output::set_data_value(remote_json(&add_args.name, &config));
            Ok(())
        }
        RemoteCommand::List(_) => {
            let remotes = list_remote_names(&repo_path)?
                .into_iter()
                .map(|name| {
                    let config = read_remote_config(&repo_path, &name)?;
                    Ok(remote_json(&name, &config))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            josh_cli::output::set_data_value(serde_json::Value::Array(remotes.clone()));
            if remotes.is_empty() {
                println!("No Josh remotes configured.");
            } else {
                for remote in &remotes {
                    println!(
                        "{}\t{}\t{}",
                        remote["name"].as_str().unwrap_or_default(),
                        remote["filter"].as_str().unwrap_or_default(),
                        remote["url"].as_str().unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        RemoteCommand::Show(show_args) => {
            let config = read_remote_config(&repo_path, &show_args.name)
                .with_context(|| format!("Remote '{}' not found", show_args.name))?;
            let data = remote_json(&show_args.name, &config);
            josh_cli::output::set_data_value(data.clone());
            println!("Remote: {}", show_args.name);
            println!("URL: {}", josh_cli::output::sanitize(&config.url));
            println!(
                "Filter: {}",
                josh_core::filter::spec(config.filter_with_meta.peel())
            );
            println!("Fetch: {}", config.ref_spec);
            println!(
                "Forge: {}",
                config
                    .forge
                    .map(|forge| forge.to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            Ok(())
        }
        RemoteCommand::SetFilter(set_args) => {
            let config = read_remote_config(&repo_path, &set_args.name)
                .with_context(|| format!("Remote '{}' not found", set_args.name))?;
            let filter = josh_core::filter::parse(&set_args.filter)
                .with_context(|| format!("Invalid filter '{}'", set_args.filter))?;
            josh_core::filter::invert(filter)
                .with_context(|| format!("Filter '{}' is not reversible", set_args.filter))?;
            let canonical = josh_core::filter::spec(filter);
            let default_branch = if set_args.apply {
                Some(josh_cli::remote_ops::resolve_default_branch(
                    repo,
                    &set_args.name,
                )?)
            } else {
                None
            };

            write_remote_config(
                &repo_path,
                &set_args.name,
                &config.url,
                &canonical,
                &config.ref_spec,
                config.forge,
            )?;
            if let Some(default_branch) = &default_branch {
                josh_cli::remote_ops::apply_josh_filtering(
                    transaction,
                    filter,
                    &set_args.name,
                    default_branch,
                )?;
            }
            josh_cli::output::set_data_value(serde_json::json!({
                "action": "set-filter",
                "remote": set_args.name,
                "filter": canonical,
                "filter_id": filter.id().to_string(),
                "applied": set_args.apply,
            }));
            println!(
                "Set filter for remote '{}' to '{}'{}",
                set_args.name,
                canonical,
                if set_args.apply {
                    " and updated filtered refs"
                } else {
                    ""
                }
            );
            Ok(())
        }
        RemoteCommand::Remove(remove_args) => {
            let config = read_remote_config(&repo_path, &remove_args.name)
                .with_context(|| format!("Remote '{}' not found", remove_args.name))?;
            let data = serde_json::json!({
                "action": "remove",
                "dry_run": remove_args.dry_run,
                "remote": remote_json(&remove_args.name, &config),
            });
            josh_cli::output::set_data_value(data);
            if remove_args.dry_run {
                println!("Would remove Josh remote '{}'", remove_args.name);
                return Ok(());
            }

            if repo.find_remote(&remove_args.name).is_ok() {
                transaction
                    .spawn_git(&["remote", "remove", &remove_args.name], &[])
                    .context("Failed to remove Git remote")?;
            }
            remove_private_remote_refs(repo, &remove_args.name)?;
            remove_remote_config(&repo_path, &remove_args.name)?;
            println!("Removed Josh remote '{}'", remove_args.name);
            Ok(())
        }
    }
}

fn remote_json(name: &str, config: &RemoteConfig) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "url": josh_cli::output::sanitize(&config.url),
        "filter": josh_core::filter::spec(config.filter_with_meta.peel()),
        "fetch": config.ref_spec,
        "forge": config.forge.map(|forge| forge.to_string()),
    })
}

fn remove_private_remote_refs(repo: &git2::Repository, remote: &str) -> anyhow::Result<()> {
    let prefixes = [
        format!("refs/josh/remotes/{remote}/"),
        format!("refs/remotes/{remote}/"),
        format!("refs/namespaces/josh-{remote}/"),
    ];
    let names = repo
        .references()?
        .filter_map(Result::ok)
        .filter_map(|reference| reference.name().map(ToOwned::to_owned))
        .filter(|name| prefixes.iter().any(|prefix| name.starts_with(prefix)))
        .collect::<Vec<_>>();
    for name in names {
        if let Ok(mut reference) = repo.find_reference(&name) {
            reference.delete()?;
        }
    }
    Ok(())
}

fn handle_remote_add_repo(args: &RemoteAddArgs, repo_path: &std::path::Path) -> anyhow::Result<()> {
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;
    let workdir = normalize_repo_path(repo_path);

    let config_path = josh_cli::config::remote_config_path(repo_path, &args.name)?;
    if config_path.exists()
        || repo.find_remote(&args.name).is_ok()
        || list_remote_names(repo_path)?
            .iter()
            .any(|name| name == &args.name)
    {
        return Err(anyhow::anyhow!(
            "Remote '{}' already exists; remove it before adding it again",
            args.name
        ));
    }

    let remote_url = to_absolute_remote_url(&args.url)?;
    let filter_to_store = args.filter.clone();
    josh_core::filter::parse(&filter_to_store)
        .with_context(|| format!("Invalid filter '{}'", filter_to_store))?;
    let refspec = format!("+refs/heads/*:refs/josh/remotes/{}/*", args.name);
    let forge = if args.forge_args.no_forge {
        None
    } else {
        args.forge_args
            .forge
            .or_else(|| josh_cli::forge::guess_forge(&remote_url))
    };

    // Set up the Git transport first and roll it back if a later step fails.
    let repo_remote = to_absolute_remote_url(&workdir.display().to_string())?;
    spawn_git_command(
        repo.path(),
        &["remote", "add", &args.name, &repo_remote],
        &[],
    )
    .context("Failed to add git remote")?;

    let namespace = format!("josh-{}", args.name);
    let uploadpack_cmd = format!("env GIT_NAMESPACE={} git upload-pack", namespace);
    let setup = (|| -> anyhow::Result<()> {
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
        write_remote_config(
            repo_path,
            &args.name,
            &remote_url,
            &filter_to_store,
            &refspec,
            forge,
        )
        .context("Failed to write remote config file")?;
        Ok(())
    })();
    if let Err(error) = setup {
        let _ = spawn_git_command(repo.path(), &["remote", "remove", &args.name], &[]);
        let _ = std::fs::remove_file(config_path);
        return Err(error);
    }

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
    if let Some(command) = &args.command {
        return handle_filter_inspect(command);
    }

    let remote = args
        .remote
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("A remote or filter subcommand is required"))?;
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let RemoteConfig {
        filter_with_meta, ..
    } = read_remote_config(&repo_path, remote)
        .with_context(|| format!("Failed to read remote config for '{}'", remote))?;

    let filter = filter_with_meta.peel();
    let filter_str = josh_core::filter::spec(filter);

    println!("Applying filter '{}' to remote '{}'", filter_str, remote);
    let default_branch = josh_cli::remote_ops::resolve_default_branch(repo, remote)?;
    josh_cli::remote_ops::apply_josh_filtering(transaction, filter, remote, &default_branch)?;

    println!("Applied filter '{}' to remote '{}'", filter_str, remote);
    josh_cli::output::set_data_value(serde_json::json!({
        "remote": remote,
        "filter": filter_str,
        "default_branch": default_branch,
    }));
    Ok(())
}

fn handle_filter_inspect(command: &FilterCommand) -> anyhow::Result<()> {
    let (action, args) = match command {
        FilterCommand::Validate(args) => ("validate", args),
        FilterCommand::Explain(args) => ("explain", args),
    };
    let filter = josh_core::filter::parse(&args.filter)
        .with_context(|| format!("Invalid filter '{}'", args.filter))?;
    let inverse = josh_core::filter::invert(filter)
        .with_context(|| format!("Filter '{}' is not reversible", args.filter))?;
    let canonical = josh_core::filter::pretty(filter, 0);
    let inverse_canonical = josh_core::filter::pretty(inverse, 0);
    josh_cli::output::set_data_value(serde_json::json!({
        "action": action,
        "valid": true,
        "reversible": true,
        "filter": canonical,
        "filter_id": filter.id().to_string(),
        "inverse": inverse_canonical,
        "inverse_id": inverse.id().to_string(),
    }));

    if action == "validate" {
        println!("valid\t{}", canonical);
    } else {
        println!("Filter: {}", canonical);
        println!("ID: {}", filter.id());
        println!("Inverse: {}", inverse_canonical);
        println!("Inverse ID: {}", inverse.id());
    }
    Ok(())
}
