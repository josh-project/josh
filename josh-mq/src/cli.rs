use clap::Parser;

#[derive(Parser)]
#[command(about = "Josh Merge Queue")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    /// Initialize metarepo
    Init,
    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Fetch remotes, collect and record state of conditions
    Fetch,
    /// Single step through the queue, updating the state
    Step,
    /// Push updated metarepo state to remotes
    Push,
}

#[derive(clap::Subcommand)]
pub enum ConfigCommands {
    /// Manage remotes
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },
}

#[derive(clap::Subcommand)]
pub enum RemoteCommands {
    /// Add a remote
    Add {
        /// Remote name
        name: String,
        /// Remote URL
        url: String,
        /// Main branch name
        #[arg(long, default_value = "main")]
        main: String,
        /// Credential string
        #[arg(long, default_value = "")]
        credential: String,
    },
    /// Remove a remote
    Remove {
        /// Remote name
        name: String,
    },
}
