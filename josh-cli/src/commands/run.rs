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
}

pub fn handle_compose(
    args: &ComposeArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        ComposeCommand::Run(run_args) => handle_run(run_args, transaction),
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

    /// Git ref to use as input: "." (working tree), "+" (index), "HEAD", or any ref
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
