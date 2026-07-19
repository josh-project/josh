use anyhow::Context;
use clap::Subcommand;

use crate::cli_println as println;
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
    /// Debug forge authentication
    Debug(ForgeArgs),
}

#[derive(Debug, clap::Parser)]
pub struct ForgeArgs {
    /// Forge to authenticate with
    #[arg()]
    pub forge: Forge,
}

pub fn handle_auth(args: &AuthArgs) -> anyhow::Result<()> {
    let (action, forge) = match &args.command {
        AuthCommand::Login(forge_args) => {
            if crate::output::is_non_interactive() {
                anyhow::bail!(
                    "GitHub device login is interactive; run without --non-interactive or \
                     configure the GH_TOKEN environment variable"
                );
            }
            match forge_args.forge {
                Forge::Github => {
                    let rt =
                        tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
                    rt.block_on(crate::forge::github::login())?;
                }
            }
            ("login", forge_args.forge)
        }
        AuthCommand::Logout(forge_args) => {
            match forge_args.forge {
                Forge::Github => crate::forge::github::logout()?,
            }
            println!("Logged out from {}.", forge_args.forge);
            ("logout", forge_args.forge)
        }
        AuthCommand::Debug(forge_args) => {
            match forge_args.forge {
                Forge::Github => handle_debug_github_auth()?,
            }
            ("debug", forge_args.forge)
        }
    };
    crate::output::set_data_value(serde_json::json!({
        "action": action,
        "forge": forge.to_string(),
        "success": true,
    }));
    Ok(())
}

fn handle_debug_github_auth() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;

    rt.block_on(async {
        let api_connection = crate::forge::github::make_api_connection()
            .await
            .context(crate::forge::github::api_connection_hint())?;

        let result = api_connection
            .get_default_branch("josh-project", "josh")
            .await?;

        match result {
            Some((branch, oid)) => {
                println!(
                    "API call to get default branch succeeded: {} ({})",
                    branch, oid
                );
            }
            None => {
                println!("API call returned no data");
            }
        }

        Ok(())
    })
}
