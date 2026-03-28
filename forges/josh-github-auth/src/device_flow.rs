use anyhow::{Result, anyhow};
use reqwest::header;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

/// Step 1 response: codes for the user to enter.
#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

/// Step 3 response: the access token.
#[derive(Debug, Deserialize)]
pub struct AccessTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// Refreshed token response.
#[derive(Debug, Deserialize)]
pub struct RefreshTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeviceFlowError {
    AuthorizationPending,
    SlowDown,
    ExpiredToken,
    AccessDenied,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: DeviceFlowError,
    #[allow(dead_code)]
    error_description: Option<String>,
}

pub struct DeviceAuthFlow {
    client: reqwest::Client,
    client_id: String,
}

impl DeviceAuthFlow {
    pub fn new(client_id: impl AsRef<str>) -> Self {
        Self {
            client: reqwest::Client::new(),
            client_id: client_id.as_ref().into(),
        }
    }

    async fn send_form(&self, url: &str, params: &[(&str, &str)]) -> Result<String> {
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();

        let resp = self
            .client
            .post(url)
            .header(header::ACCEPT, mime::APPLICATION_JSON.as_ref())
            .header(
                header::CONTENT_TYPE,
                mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
            )
            .body(body)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(anyhow!("request to {} failed ({}): {}", url, status, body));
        }

        Ok(body)
    }

    /// Step 1: Request device and user codes from GitHub.
    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let body = self
            .send_form(
                GITHUB_DEVICE_CODE_URL,
                &[
                    ("client_id", &self.client_id),
                    ("scope", &"repo workflow".to_string()),
                ],
            )
            .await?;

        Ok(serde_json::from_str(&body)?)
    }

    /// Step 2+3: Poll GitHub until the user authorizes the device.
    ///
    /// This blocks (async sleep) respecting the `interval` from the device code
    /// response. Backs off on `slow_down` and stops on `expired_token` or
    /// `access_denied`.
    ///
    /// An optional `notify` receiver can be passed to signal the poll loop to
    /// check immediately instead of waiting for the full interval to elapse.
    pub async fn poll_for_token(
        &self,
        device_code: &DeviceCodeResponse,
        mut notify: Option<mpsc::UnboundedReceiver<()>>,
    ) -> Result<AccessTokenResponse> {
        let mut interval = Duration::from_secs(device_code.interval);

        loop {
            match &mut notify {
                Some(rx) => {
                    tokio::select! {
                        _ = tokio::time::sleep(interval) => {}
                        _ = rx.recv() => {}
                    }
                }
                None => {
                    tokio::time::sleep(interval).await;
                }
            }

            let body = self
                .send_form(
                    GITHUB_ACCESS_TOKEN_URL,
                    &[
                        ("client_id", self.client_id.as_str()),
                        ("device_code", &device_code.device_code),
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ],
                )
                .await?;

            // GitHub returns 200 even for pending/error states — check for
            // the `error` field to distinguish.
            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
                match err.error {
                    DeviceFlowError::AuthorizationPending => {
                        tracing::debug!("authorization pending, polling again");
                        continue;
                    }
                    DeviceFlowError::SlowDown => {
                        interval += Duration::from_secs(5);
                        tracing::debug!("slow_down received, interval now {:?}", interval);
                        continue;
                    }
                    DeviceFlowError::ExpiredToken => {
                        return Err(anyhow!("device code expired, please restart the flow"));
                    }
                    DeviceFlowError::AccessDenied => {
                        return Err(anyhow!("user denied authorization"));
                    }
                    DeviceFlowError::Unknown => {
                        return Err(anyhow!("unexpected error from GitHub: {}", body));
                    }
                }
            }

            return Ok(serde_json::from_str(&body)?);
        }
    }

    /// Refresh an expired access token using a refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<RefreshTokenResponse> {
        let body = self
            .send_form(
                GITHUB_ACCESS_TOKEN_URL,
                &[
                    ("client_id", self.client_id.as_str()),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                ],
            )
            .await?;

        Ok(serde_json::from_str(&body)?)
    }
}
