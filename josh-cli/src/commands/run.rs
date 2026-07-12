use josh_compose::{CleanMode, RunOptions};

#[derive(Debug, clap::Parser)]
pub struct ComposeArgs {
    #[command(subcommand)]
    pub command: ComposeCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum ComposeCommand {
    /// Run a workspace in a container
    Run(RunArgs),
    /// List every image (as `josh_ws_image_<oid>`) a `run` with the same args would need
    ListImages(ListImagesArgs),
    /// List the job hash of every workspace a `run` with the same args would touch
    ListJobs(ListJobsArgs),
}

pub fn handle_compose(
    args: &ComposeArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        ComposeCommand::Run(run_args) => handle_run(run_args, transaction),
        ComposeCommand::ListImages(list_args) => handle_list_images(list_args, transaction),
        ComposeCommand::ListJobs(list_args) => handle_list_jobs(list_args, transaction),
    }
}

#[derive(Debug, clap::Parser)]
pub struct RunArgs {
    /// Remove cached images and output volumes
    #[arg(long = "clean")]
    pub clean: bool,

    /// Remove cached images, output volumes, and persistent cache volumes
    #[arg(long = "clean-all")]
    pub clean_all: bool,

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

    josh_compose::run(
        transaction,
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean,
        },
    )
}

#[derive(Debug, clap::Parser)]
pub struct ListImagesArgs {
    /// Ignore the local job cache and list every image a fresh run would build
    #[arg(long = "all")]
    pub all: bool,

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
    let oids = josh_compose::plan_images(
        transaction,
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean: CleanMode::None,
        },
        args.all,
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
    let oids = josh_compose::plan_jobs(
        transaction,
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean: CleanMode::None,
        },
        args.all,
    )?;

    for oid in oids {
        println!("{oid}");
    }
    Ok(())
}
