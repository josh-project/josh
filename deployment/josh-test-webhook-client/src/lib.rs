use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use backon::{ExponentialBuilder, Retryable};
use futures_util::{SinkExt, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use josh_test_webhook_service::WebhookPayload;

#[derive(Clone)]
pub struct WebhookClientConfig {
    pub ws_url: String,
    pub auth_token: String,
    pub webhook_url: String,
}

pub struct WebhookClientHandle {
    ping_task: tokio::task::JoinHandle<()>,
    forward_task: tokio::task::JoinHandle<()>,
}

impl Drop for WebhookClientHandle {
    fn drop(&mut self) {
        self.ping_task.abort();
        self.forward_task.abort();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookClientError {
    #[error("WebSocket connection error: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Failed to decode webhook payload: {0}")]
    DecodeError(#[from] bincode::error::DecodeError),

    #[error("Failed to forward webhook: {0}")]
    ForwardError(#[from] reqwest::Error),

    #[error("Invalid header: {0}")]
    HeaderError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),
}

const PING_SIZE: usize = 16;
const PING_INTERVAL: Duration = Duration::from_secs(10);

async fn connect_with_retry(
    config: &WebhookClientConfig,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let ws_url = Url::parse(&config.ws_url).context("invalid WebSocket URL")?;
    let auth_token = config.auth_token.clone();

    let connect_fn = || async {
        // Create a request with tokio-tungstenite's default WebSocket headers
        let mut request =
            tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
                ws_url.as_str(),
            )?;

        // Add our custom Authorization header
        request.headers_mut().insert(
            http::header::AUTHORIZATION,
            format!("Bearer {}", auth_token).parse().unwrap(),
        );

        tokio_tungstenite::connect_async(request)
            .await
            .map(|(stream, _)| stream)
            .map_err(|e| anyhow::anyhow!("connection failed: {}", e))
    };

    let retry_policy = ExponentialBuilder::default();
    let ws_stream = connect_fn
        .retry(&retry_policy)
        .notify(|err, dur| {
            tracing::warn!("connection failed: {}, retrying in {:?}", err, dur);
        })
        .await?;

    tracing::info!("connected to webhook server");
    Ok(ws_stream)
}

pub async fn connect(config: &WebhookClientConfig) -> anyhow::Result<WebhookClientHandle> {
    let webhook_url = Url::parse(&config.webhook_url)?;

    let ws_stream = connect_with_retry(config).await?;
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let ping_task = tokio::task::spawn(async move {
        let mut interval = tokio::time::interval(PING_INTERVAL);

        loop {
            interval.tick().await;

            let ping_data = rand::random::<[u8; PING_SIZE]>().to_vec();
            let ping_data = bytes::Bytes::from(ping_data);

            let result = ws_sink.send(Message::Ping(ping_data)).await;

            match result {
                Ok(_) => continue,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to write to ws");
                    break;
                }
            }
        }
    });

    let forward_task = tokio::task::spawn(async move {
        let client = reqwest::Client::builder()
            .build()
            .expect("failed to build http client");

        while let Some(message) = ws_stream.next().await {
            let message = match message {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to read from ws");
                    break;
                }
            };

            let payload = match message {
                Message::Binary(payload) => payload,
                Message::Pong(_) => continue,
                Message::Close(_) => break,
                m => {
                    tracing::warn!(message = ?m, "unexpected message");
                    continue;
                }
            };

            let (payload, _) = match bincode::decode_from_slice::<WebhookPayload, _>(
                &payload,
                bincode::config::standard(),
            ) {
                Ok(payload) => payload,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to decode event");
                    break;
                }
            };

            match forward_webhook(&client, &webhook_url, payload).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = ?e, "failed to forward webhook");
                }
            }
        }
    });

    Ok(WebhookClientHandle {
        ping_task,
        forward_task,
    })
}

async fn forward_webhook(
    client: &reqwest::Client,
    target_url: &Url,
    payload: WebhookPayload,
) -> Result<(), WebhookClientError> {
    let mut headers = HeaderMap::new();

    // Convert headers from the webhook payload
    for (key, value) in payload.headers {
        let header_name = HeaderName::from_str(&key).map_err(|e| {
            WebhookClientError::HeaderError(format!("Invalid header name '{}': {}", key, e))
        })?;
        let header_value = HeaderValue::from_str(&value).map_err(|e| {
            WebhookClientError::HeaderError(format!("Invalid header value for '{}': {}", key, e))
        })?;
        headers.insert(header_name, header_value);
    }

    // Forward the webhook
    let response = client
        .post(target_url.as_str())
        .headers(headers)
        .body(payload.body)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::debug!("Successfully forwarded webhook to {}", target_url);
    } else {
        tracing::warn!(
            "Webhook forward returned status {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );
    }

    Ok(())
}
