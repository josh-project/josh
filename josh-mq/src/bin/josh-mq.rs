use clap::Parser;

#[derive(Parser)]
#[command(about = "Josh Merge Queue")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Initialize metarepo
    Init,
    /// Fetch remotes, collect and record state of conditions
    Fetch,
    /// Single step through the queue, updating the state
    Step,
    /// Push updated metarepo state to remotes
    Push,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            todo!()
        }
        Commands::Fetch => {
            todo!()
        }
        Commands::Step => {
            todo!()
        }
        Commands::Push => {
            todo!()
        }
    }
}
