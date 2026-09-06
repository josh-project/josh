use josh_compose::{CleanMode, RunOptions};
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
    /// Run a workspace in a container
    Run(RunArgs),
    /// Print the workspace graph as D2 source
    Graph(GraphArgs),
    /// List every image (as `josh_ws_image_<oid>`) a `run` with the same args would need
    ListImages(ListImagesArgs),
    /// List the job hash of every workspace a `run` with the same args would touch
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
        ComposeCommand::Run(run_args) => handle_run(run_args, transaction),
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
pub struct RunArgs {
    /// Remove cached images and output volumes
    #[arg(long = "clean")]
    pub clean: bool,

    /// Remove cached images, output volumes, and persistent cache volumes
    #[arg(long = "clean-all")]
    pub clean_all: bool,

    /// Container backend to run the workspace in [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,

    /// Git revision to use as input: "." (working tree), "+" (index), or any rev (e.g. "HEAD", "HEAD~1", "main")
    #[arg(default_value = ".")]
    pub reference: String,

    /// Filter spec to apply, e.g. ":+ws/test" (defaults to ":+compose")
    #[arg(default_value = ":+compose")]
    pub filter: String,
}

pub fn handle_run(
    args: &RunArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let clean = if args.clean_all {
        CleanMode::CleanAll
    } else if args.clean {
        CleanMode::Clean
    } else {
        CleanMode::None
    };

    let runtime = args.backend.unwrap_or_else(default_backend).runtime();
    josh_compose::run(
        transaction,
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean,
        },
        runtime.as_ref(),
    )
}

#[derive(Debug, clap::Parser)]
pub struct GraphArgs {
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
    let graph = josh_compose::load_plan(transaction, &args.filter, &args.reference)?;
    println!("{}", graph.d2());
    Ok(())
}

#[derive(Debug, clap::Parser)]
pub struct ListImagesArgs {
    /// Ignore the local job cache and list every image a fresh run would build
    #[arg(long = "all")]
    pub all: bool,

    /// Container backend to check for prepared images [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,

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
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean: CleanMode::None,
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
    /// Ignore the local job cache and list every job a fresh run would touch
    #[arg(long = "all")]
    pub all: bool,

    /// Container backend to check for existing outputs [default: podman, or docker on macOS when OrbStack is running]
    #[arg(long, value_enum, env = "JOSH_COMPOSE_BACKEND")]
    pub backend: Option<Backend>,

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
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean: CleanMode::None,
        },
        args.all,
        runtime.as_ref(),
    )?;

    for oid in oids {
        println!("{oid}");
    }
    Ok(())
}
