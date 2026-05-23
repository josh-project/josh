use anyhow::Context;

use josh_github_auth::APP_CLIENT_ID;
use josh_github_auth::device_flow::DeviceAuthFlow;
use josh_github_graphql::connection::GithubApiConnection;

/// Login to GitHub using device flow and store the token.
pub async fn login() -> anyhow::Result<()> {
    let flow = DeviceAuthFlow::new(APP_CLIENT_ID);

    let device_code = flow
        .request_device_code()
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

    let keyring = josh_github_keyring::default_store()?;
    let stored = josh_github_keyring::StoredToken::from(token);
    let json = serde_json::to_string(&stored).context("failed to serialize token")?;
    keyring.set_password(&json)?;

    eprintln!("Logged in to GitHub successfully.");
    Ok(())
}

pub fn logout() -> anyhow::Result<()> {
    let keyring = josh_github_keyring::default_store()?;
    keyring.delete_credential()?;

    Ok(())
}

// Matches official github CLI and other github-adjacent tools
pub const GITHUB_USER_TOKEN_ENV: &str = "GH_TOKEN";

pub async fn make_api_connection() -> Option<GithubApiConnection> {
    GithubApiConnection::from_environment()
}

pub fn api_connection_hint() -> String {
    format!(
        "Couldn't create API connection; log in to GitHub with 'josh auth login github', or set {} environment variable",
        GITHUB_USER_TOKEN_ENV
    )
}
