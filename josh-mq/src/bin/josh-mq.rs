use clap::Parser;
use josh_mq::cli::{Cli, Commands, ConfigCommands};
use josh_mq::config::{create_empty_config, handle_config_remote_command};

fn open_repo() -> anyhow::Result<gix::Repository> {
    let dir = std::env::current_dir()?;
    let repo = gix::ThreadSafeRepository::open(&dir)?.to_thread_local();

    Ok(repo)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let repo = open_repo()?;

    match cli.command {
        Commands::Init => {
            create_empty_config(&repo)?;
        }
        Commands::Config { command } => match command {
            ConfigCommands::Remote { command } => {
                handle_config_remote_command(&repo, command)?;
            }
        },
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

    Ok(())
}
