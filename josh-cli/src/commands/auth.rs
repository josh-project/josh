use anyhow::Context;
use clap::Subcommand;

use crate::forge::Forge;

#[derive(Debug, clap::Parser)]
pub struct AuthArgs {
    /// Auth action to perform
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Log in to a forge
    Login(ForgeArgs),
    /// Log out from a forge
    Logout(ForgeArgs),
}

#[derive(Debug, clap::Parser)]
pub struct ForgeArgs {
    /// Forge to authenticate with
    #[arg()]
    pub forge: Forge,
}

pub fn handle_auth(args: &AuthArgs) -> anyhow::Result<()> {
    match &args.command {
        AuthCommand::Login(forge_args) => match forge_args.forge {
            Forge::Github => {
                let rt =
                    tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
                rt.block_on(josh_github_auth::token::login())
            }
        },
        AuthCommand::Logout(forge_args) => match forge_args.forge {
            Forge::Github => josh_github_auth::token::logout(),
        },
    }
}
