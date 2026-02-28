use anyhow::{Result, anyhow};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::header;
use secret_vault_value::SecretValue;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "josh-project";
const API_VERSION_HEADER: &str = "X-GitHub-Api-Version";
const API_VERSION: &str = "2022-11-28";
const ACCEPT_HEADER: &str = "application/vnd.github+json";

/// JWT lifetime: 10 minutes (GitHub maximum).
const JWT_LIFETIME_SECS: u64 = 600;

/// Buffer before expiry to trigger a refresh.
const EXPIRY_BUFFER: Duration = Duration::from_secs(60);

#[derive(Debug, Serialize)]
struct Claims {
    iat: u64,
    exp: u64,
    iss: String,
}

struct TokenInner {
    value: String,
    expires_at: Instant,
}

pub struct GithubAppAuth {
    app_id: String,
    installation_id: String,
    private_key: SecretValue,
    client: reqwest::Client,
    cached_token: Option<TokenInner>,
}

#[derive(Debug, Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: String,
}

impl GithubAppAuth {
    /// Create a new GitHub App auth handle and fetch the initial installation token.
    pub async fn authenticate(
        app_id: String,
        installation_id: String,
        key: SecretValue,
    ) -> Result<Self> {
        let mut auth = Self {
            app_id,
            installation_id,
            private_key: key,
            client: reqwest::Client::new(),
            cached_token: None,
        };

        auth.refresh().await?;
        Ok(auth)
    }

    /// Return a valid installation token, refreshing if the cached one is stale.
    pub async fn get_or_refresh(&mut self) -> Result<String> {
        if let Some(ref inner) = self.cached_token {
            if Instant::now() + EXPIRY_BUFFER < inner.expires_at {
                return Ok(inner.value.clone());
            }
        }

        self.refresh().await
    }

    async fn refresh(&mut self) -> Result<String> {
        let jwt = self.generate_jwt()?;
        let resp = self.request_token(&jwt).await?;

        let expires_at = parse_expires_at(&resp.expires_at)?;

        self.cached_token = Some(TokenInner {
            value: resp.token.clone(),
            expires_at,
        });

        Ok(resp.token)
    }

    fn generate_jwt(&self) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow!("system time error: {}", e))?
            .as_secs();

        let claims = Claims {
            iat: now.saturating_sub(60),
            exp: now + JWT_LIFETIME_SECS,
            iss: self.app_id.clone(),
        };

        let key = EncodingKey::from_rsa_pem(self.private_key.as_sensitive_bytes())
            .map_err(|e| anyhow!("invalid RSA private key: {}", e))?;

        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| anyhow!("JWT encoding failed: {}", e))
    }

    async fn request_token(&self, jwt: &str) -> Result<InstallationTokenResponse> {
        let url = format!(
            "{}/app/installations/{}/access_tokens",
            GITHUB_API_BASE, self.installation_id
        );

        let resp = self
            .client
            .post(&url)
            .header(header::AUTHORIZATION, format!("Bearer {}", jwt))
            .header(header::ACCEPT, ACCEPT_HEADER)
            .header(header::USER_AGENT, USER_AGENT)
            .header(API_VERSION_HEADER, API_VERSION)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(anyhow!(
                "failed to get installation token ({}): {}",
                status,
                body
            ));
        }

        let parsed: InstallationTokenResponse = serde_json::from_str(&body)?;
        Ok(parsed)
    }
}

/// Parse GitHub's ISO 8601 `expires_at` into a tokio `Instant`.
fn parse_expires_at(expires_at: &str) -> Result<Instant> {
    // GitHub returns e.g. "2024-01-01T01:00:00Z"
    let ts: chrono::DateTime<chrono::Utc> = expires_at
        .parse()
        .map_err(|e| anyhow!("failed to parse expires_at '{}': {}", expires_at, e))?;

    let expires_epoch = ts.timestamp() as u64;
    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow!("system time error: {}", e))?
        .as_secs();

    if expires_epoch <= now_epoch {
        return Err(anyhow!(
            "installation token already expired at {}",
            expires_at
        ));
    }

    let remaining = Duration::from_secs(expires_epoch - now_epoch);
    Ok(Instant::now() + remaining)
}
