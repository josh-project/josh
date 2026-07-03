use anyhow::Context;

use josh_github_auth::APP_CLIENT_ID;
use josh_github_auth::device_flow::DeviceAuthFlow;

pub use josh_github_changes::connection::{
    GITHUB_USER_TOKEN_ENV, api_connection_hint, make_api_connection,
};

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
