use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::Subcommand;

use crate::forge::{Forge, GITHUB_APP_CLIENT_ID};

const KEYRING_SERVICE: &str = "josh-cli";
const KEYRING_GITHUB_ACCESS_TOKEN: &str = "github:access_token";
const KEYRING_GITHUB_REFRESH_TOKEN: &str = "github:refresh_token";
const KEYRING_GITHUB_TOKEN_EXPIRY: &str = "github:token_expiry";

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
            Forge::Github => github_login(),
        },
        AuthCommand::Logout(forge_args) => match forge_args.forge {
            Forge::Github => github_logout(),
        },
    }
}

fn github_login() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;

    rt.block_on(async {
        let flow =
            josh_github_auth::device_flow::DeviceAuthFlow::new(GITHUB_APP_CLIENT_ID.to_string());

        let device_code = flow
            .request_device_code("repo")
            .await
            .context("failed to request device code")?;

        let url = device_code
            .verification_uri_complete
            .as_deref()
            .unwrap_or(&device_code.verification_uri);

        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&device_code.user_code)) {
            Ok(()) => eprintln!("Code copied to clipboard: {}", device_code.user_code),
            Err(_) => eprintln!("Enter code: {}", device_code.user_code),
        }

        eprintln!("Open this URL in your browser: {}", url);
        eprintln!(
            "Waiting for authorization (expires in {}s)...",
            device_code.expires_in
        );

        let token = flow
            .poll_for_token(&device_code, None)
            .await
            .context("failed to complete device authorization")?;

        github_store_keyring(&token)?;

        eprintln!("Logged in to GitHub successfully.");
        Ok(())
    })
}

fn github_store_keyring(
    token: &josh_github_auth::device_flow::AccessTokenResponse,
) -> anyhow::Result<()> {
    fn store_entry(key: &str, value: &str) -> anyhow::Result<()> {
        keyring::Entry::new(KEYRING_SERVICE, key)
            .context("failed to create keyring entry")?
            .set_password(value)
            .with_context(|| format!("failed to store {} in keyring", key))
    }

    store_entry(KEYRING_GITHUB_ACCESS_TOKEN, &token.access_token)?;

    if let Some(expires_in) = token.expires_in {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock error")?
            .as_secs()
            + expires_in;

        store_entry(KEYRING_GITHUB_TOKEN_EXPIRY, &expiry.to_string())?;
    }

    if let Some(ref refresh_token) = token.refresh_token {
        store_entry(KEYRING_GITHUB_REFRESH_TOKEN, refresh_token)?;
    }

    Ok(())
}

fn github_logout() -> anyhow::Result<()> {
    let mut cleared = false;

    for key in [
        KEYRING_GITHUB_ACCESS_TOKEN,
        KEYRING_GITHUB_REFRESH_TOKEN,
        KEYRING_GITHUB_TOKEN_EXPIRY,
    ] {
        let entry =
            keyring::Entry::new(KEYRING_SERVICE, key).context("failed to create keyring entry")?;

        match entry.delete_credential() {
            Ok(()) => cleared = true,
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e).context(format!("failed to delete {}", key)),
        }
    }

    if cleared {
        eprintln!("Logged out from GitHub.");
    } else {
        eprintln!("No GitHub credentials found.");
    }

    Ok(())
}
