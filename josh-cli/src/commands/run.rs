use josh_run::{CleanMode, RunOptions};

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

    /// Filter spec to apply, e.g. ":+ws/test" (defaults to ":+run")
    #[arg(default_value = ":+run")]
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

    josh_run::run(
        transaction,
        RunOptions {
            filter_spec: args.filter.clone(),
            input_ref: args.reference.clone(),
            clean,
        },
    )
}
