use josh_compose::{ArgumentBinding, CleanMode, WorkspaceOptions};
use josh_compose_backend::Runtime;
use josh_compose_docker::DockerRuntime;
use josh_compose_podman::PodmanRuntime;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Backend {
    Podman,
    Docker,
}

impl Backend {
    fn runtime(&self) -> Box<dyn Runtime> {
        match self {
            Backend::Podman => Box::new(PodmanRuntime::new()),
            Backend::Docker => Box::new(DockerRuntime::new()),
        }
    }
}

/// Backend used when `--backend`/`JOSH_COMPOSE_BACKEND` is not given: podman,
/// except on macOS with OrbStack running, where docker is preferred.
fn default_backend() -> Backend {
    #[cfg(target_os = "macos")]
    if orbstack_running() {
        return Backend::Docker;
    }
    Backend::Podman
}

#[cfg(target_os = "macos")]
fn orbstack_running() -> bool {
    std::process::Command::new("orbctl")
        .arg("status")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "Running"
        })
        .unwrap_or(false)
}

#[derive(Debug, clap::Parser)]
pub struct ComposeArgs {
    #[command(subcommand)]
    pub command: ComposeCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ComposeCommand {
    /// Ensure a workspace result exists, using cached results when available
    Build(TargetArgs),
    /// Execute the selected workspace now; dependencies still use cached results
    Run(RunArgs),
    /// Open an interactive shell in a prepared workspace
    Shell(ShellArgs),
    /// Remove compose results and runtime artifacts
    Clean(CleanArgs),
    /// Print the workspace graph as D2 source
    Graph(GraphArgs),
    /// List every image (as `josh_ws_image_<oid>`) a `build` with the same args would need
    ListImages(ListImagesArgs),
    /// List the job hash of every workspace a `build` with the same args would touch
    ListJobs(ListJobsArgs),
    /// Pull compose result metadata from a Git remote
    Pull(TransferArgs),
    /// Push compose result metadata to a Git remote
    Push(TransferArgs),
}

pub fn handle_compose(
    args: &ComposeArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let _ = (args, transaction);
        anyhow::bail!("josh compose is not supported on Windows");
    }

    #[cfg(not(windows))]
    match &args.command {
        ComposeCommand::Build(build_args) => handle_build(build_args, transaction),
        ComposeCommand::Run(run_args) => handle_run(run_args, transaction),
        ComposeCommand::Shell(shell_args) => handle_shell(shell_args, transaction),
        ComposeCommand::Clean(clean_args) => handle_clean(clean_args, transaction),
        ComposeCommand::Graph(graph_args) => handle_graph(graph_args, transaction),
        ComposeCommand::ListImages(list_args) => handle_list_images(list_args, transaction),
        ComposeCommand::ListJobs(list_args) => handle_list_jobs(list_args, transaction),
        ComposeCommand::Pull(transfer_args) => {
            josh_compose::pull(transaction, &transfer_args.remote)
        }
        ComposeCommand::Push(transfer_args) => {
            josh_compose::push(transaction, &transfer_args.remote)
        }
    }
}

#[derive(Debug, clap::Parser)]
pub struct TransferArgs {
    /// Remote name or URL
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,
}

#[derive(Debug, clap::Parser)]
pub struct TargetArgs {
    /// Container backend [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,

    /// Bind a named compose argument (currently a Git revision)
    #[arg(long = "arg", value_name = "NAME=VALUE")]
    pub arguments: Vec<ArgumentBinding>,

    /// Git revision to use as input: "." (working tree), "+" (index), or any rev
    #[arg(default_value = ".")]
    pub reference: String,

    /// Filter spec to apply, e.g. ":+ws/test" (defaults to ":+compose")
    #[arg(default_value = ":+compose")]
    pub filter: String,
}

impl TargetArgs {
    fn options(&self) -> WorkspaceOptions {
        WorkspaceOptions {
            filter_spec: self.filter.clone(),
            input_ref: self.reference.clone(),
            arguments: self.arguments.clone(),
        }
    }

    fn runtime(&self) -> Box<dyn Runtime> {
        self.backend.unwrap_or_else(default_backend).runtime()
    }
}

#[derive(Debug, clap::Parser)]
pub struct RunArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Replace the configured workspace command without updating its cached result
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct ShellArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Interactive command to execute [default: /bin/sh]
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, clap::Parser)]
pub struct CleanArgs {
    /// Also remove persistent cache volumes
    #[arg(long = "all")]
    pub all: bool,

    /// Container backend [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,
}

pub fn handle_build(
    args: &TargetArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let runtime = args.runtime();
    josh_compose::build(transaction, args.options(), runtime.as_ref())
}

pub fn handle_run(
    args: &RunArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let runtime = args.target.runtime();
    let command = (!args.command.is_empty()).then(|| args.command.clone());
    josh_compose::run(
        transaction,
        args.target.options(),
        command,
        runtime.as_ref(),
    )
}

pub fn handle_shell(
    args: &ShellArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let runtime = args.target.runtime();
    let command = if args.command.is_empty() {
        vec!["/bin/sh".to_string()]
    } else {
        args.command.clone()
    };
    josh_compose::shell(
        transaction,
        args.target.options(),
        command,
        runtime.as_ref(),
    )
}

pub fn handle_clean(
    args: &CleanArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let runtime = args.backend.unwrap_or_else(default_backend).runtime();
    let mode = if args.all {
        CleanMode::CleanAll
    } else {
        CleanMode::Clean
    };
    josh_compose::clean::clean(transaction, mode, runtime.as_ref())
}

#[derive(Debug, clap::Parser)]
pub struct GraphArgs {
    /// Bind a named compose argument (currently a Git revision)
    #[arg(long = "arg", value_name = "NAME=VALUE")]
    pub arguments: Vec<ArgumentBinding>,

    /// Git revision to use as input: "." (working tree), "+" (index), or any rev (e.g. "HEAD", "HEAD~1", "main")
    #[arg(default_value = ".")]
    pub reference: String,

    /// Filter spec to apply, e.g. ":+ws/test" (defaults to ":+compose")
    #[arg(default_value = ":+compose")]
    pub filter: String,
}

pub fn handle_graph(
    args: &GraphArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let graph =
        josh_compose::load_plan(transaction, &args.filter, &args.reference, &args.arguments)?;
    println!("{}", graph.d2());
    Ok(())
}

#[derive(Debug, clap::Parser)]
pub struct ListImagesArgs {
    /// Ignore the local job cache and list every image a fresh build would create
    #[arg(long = "all")]
    pub all: bool,

    /// Container backend to check for prepared images [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,
    /// Bind a named compose argument (currently a Git revision)
    #[arg(long = "arg", value_name = "NAME=VALUE")]
    pub arguments: Vec<ArgumentBinding>,

    /// Git revision to use as input: "." (working tree), "+" (index), or any rev (e.g. "HEAD", "HEAD~1", "main")
    #[arg(default_value = ".")]
    pub reference: String,

    /// Filter spec to apply, e.g. ":+ws/test" (defaults to ":+compose")
    #[arg(default_value = ":+compose")]
    pub filter: String,
}

pub fn handle_list_images(
    args: &ListImagesArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let runtime = args.backend.unwrap_or_else(default_backend).runtime();
    let oids = josh_compose::plan_images(
        transaction,
        WorkspaceOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            arguments: args.arguments.clone(),
        },
        args.all,
        runtime.as_ref(),
    )?;

    for oid in oids {
        println!("{}", josh_compose::naming::env(oid));
    }
    Ok(())
}

#[derive(Debug, clap::Parser)]
pub struct ListJobsArgs {
    /// Ignore the local job cache and list every job a fresh build would touch
    #[arg(long = "all")]
    pub all: bool,

    /// Container backend to check for existing outputs [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,
    /// Bind a named compose argument (currently a Git revision)
    #[arg(long = "arg", value_name = "NAME=VALUE")]
    pub arguments: Vec<ArgumentBinding>,

    /// Git revision to use as input: "." (working tree), "+" (index), or any rev (e.g. "HEAD", "HEAD~1", "main")
    #[arg(default_value = ".")]
    pub reference: String,

    /// Filter spec to apply, e.g. ":+ws/test" (defaults to ":+compose")
    #[arg(default_value = ":+compose")]
    pub filter: String,
}

pub fn handle_list_jobs(
    args: &ListJobsArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let runtime = args.backend.unwrap_or_else(default_backend).runtime();
    let oids = josh_compose::plan_jobs(
        transaction,
        WorkspaceOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            arguments: args.arguments.clone(),
        },
        args.all,
        runtime.as_ref(),
    )?;

    for oid in oids {
        println!("{oid}");
    }
    Ok(())
}
