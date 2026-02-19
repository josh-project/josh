use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;

use crate::APP_CLIENT_ID;
use crate::device_flow::{AccessTokenResponse, DeviceAuthFlow};

const KEYRING_SERVICE: &str = "josh-cli";
const KEYRING_ACCESS_TOKEN: &str = "github:access_token";
const KEYRING_REFRESH_TOKEN: &str = "github:refresh_token";
const KEYRING_TOKEN_EXPIRY: &str = "github:token_expiry";

/// Read the GitHub access token: prefers GITHUB_TOKEN env (e.g. PAT with full permissions),
/// then the token stored by `josh auth login github` (keyring).
/// Use GITHUB_TOKEN if you get "Resource not accessible by integration" when creating/updating PRs
/// (the app token from device flow may lack pull request write permission).
pub fn get_access_token() -> anyhow::Result<Option<String>> {
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        if !t.trim().is_empty() {
            return Ok(Some(t));
        }
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCESS_TOKEN)
        .context("Failed to create keyring entry")?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("Failed to read GitHub access token from keyring"),
    }
}

/// Login to GitHub using device flow and store the token in the keyring.
pub async fn login() -> anyhow::Result<()> {
    let flow = DeviceAuthFlow::new(APP_CLIENT_ID);

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

    store_keyring(&token)?;

    eprintln!("Logged in to GitHub successfully.");
    Ok(())
}

fn store_keyring(token: &AccessTokenResponse) -> anyhow::Result<()> {
    fn store_entry(key: &str, value: &str) -> anyhow::Result<()> {
        keyring::Entry::new(KEYRING_SERVICE, key)
            .context("failed to create keyring entry")?
            .set_password(value)
            .with_context(|| format!("failed to store {} in keyring", key))
    }

    store_entry(KEYRING_ACCESS_TOKEN, &token.access_token)?;

    if let Some(expires_in) = token.expires_in {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock error")?
            .as_secs()
            + expires_in;

        store_entry(KEYRING_TOKEN_EXPIRY, &expiry.to_string())?;
    }

    if let Some(ref refresh_token) = token.refresh_token {
        store_entry(KEYRING_REFRESH_TOKEN, refresh_token)?;
    }

    Ok(())
}

/// Logout from GitHub by removing tokens from the keyring.
pub fn logout() -> anyhow::Result<()> {
    let mut cleared = false;

    for key in [
        KEYRING_ACCESS_TOKEN,
        KEYRING_REFRESH_TOKEN,
        KEYRING_TOKEN_EXPIRY,
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
