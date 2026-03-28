use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use josh_github_auth::APP_CLIENT_ID;
use josh_github_auth::device_flow::{AccessTokenResponse, DeviceAuthFlow};
use josh_github_auth::middleware::GithubAuthMiddleware;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::request::GITHUB_GRAPHQL_API_URL;

#[derive(Debug, Serialize, Deserialize)]
struct StoredToken {
    access_token: String,
    token_type: String,
    scope: String,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

impl From<AccessTokenResponse> for StoredToken {
    fn from(resp: AccessTokenResponse) -> Self {
        let expires_at = resp
            .expires_in
            .and_then(|secs| Duration::try_seconds(secs as i64))
            .map(|d| Utc::now() + d);

        Self {
            access_token: resp.access_token,
            token_type: resp.token_type,
            scope: resp.scope,
            refresh_token: resp.refresh_token,
            expires_at,
        }
    }
}

impl From<StoredToken> for AccessTokenResponse {
    fn from(stored: StoredToken) -> Self {
        let expires_in = stored.expires_at.map(|at| {
            let remaining = at - Utc::now();
            remaining.num_seconds().max(0) as u64
        });

        Self {
            access_token: stored.access_token,
            token_type: stored.token_type,
            scope: stored.scope,
            refresh_token: stored.refresh_token,
            expires_in,
        }
    }
}

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

    let keyring = crate::keyring::default_store()?;
    let stored = StoredToken::from(token);
    let json = serde_json::to_string(&stored).context("failed to serialize token")?;
    keyring.set_password(&json)?;

    eprintln!("Logged in to GitHub successfully.");
    Ok(())
}

pub fn load_stored_token() -> Option<AccessTokenResponse> {
    let keyring = crate::keyring::default_store().ok()?;
    let json = keyring.get_password().ok()?;
    let stored: StoredToken = serde_json::from_str(&json).ok()?;
    Some(AccessTokenResponse::from(stored))
}

pub fn logout() -> anyhow::Result<()> {
    let keyring = crate::keyring::default_store()?;
    keyring.delete_credential()?;

    Ok(())
}

// Matches official github CLI and other github-adjacent tools
pub const GITHUB_USER_TOKEN_ENV: &str = "GH_TOKEN";

pub async fn make_api_connection() -> Option<GithubApiConnection> {
    let middleware = if let Ok(token) = std::env::var(GITHUB_USER_TOKEN_ENV) {
        GithubAuthMiddleware::from_token(token)
    } else {
        let stored = load_stored_token()?;
        GithubAuthMiddleware::from_app_flow(stored, APP_CLIENT_ID.to_string())
    };

    let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
        .with(middleware)
        .build();

    Some(GithubApiConnection {
        client,
        api_url: GITHUB_GRAPHQL_API_URL
            .parse()
            .expect("Failed to parse API URL"),
    })
}

pub fn api_connection_hint() -> String {
    format!(
        "Couldn't create API connection; log in to GitHub with 'josh auth login github', or set {} environment variable",
        GITHUB_USER_TOKEN_ENV
    )
}
