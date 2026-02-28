use anyhow::Result;
use reqwest::header;
use reqwest_middleware::{Middleware, Next};
use secret_vault_value::SecretValue;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use std::time::Duration;

use crate::app_flow::GithubAppAuth;
use crate::device_flow::{AccessTokenResponse, DeviceAuthFlow};

struct TokenState {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<Instant>,
    flow: DeviceAuthFlow,
}

enum Command {
    GetToken(oneshot::Sender<Result<String>>),
}

const EXPIRY_BUFFER: Duration = Duration::from_secs(30);

async fn device_flow_actor_loop(mut state: TokenState, mut rx: mpsc::UnboundedReceiver<Command>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::GetToken(reply) => {
                let result = maybe_refresh_and_get_token(&mut state).await;
                let _ = reply.send(result);
            }
        }
    }
}

async fn maybe_refresh_and_get_token(state: &mut TokenState) -> Result<String> {
    let needs_refresh = match (state.expires_at, &state.refresh_token) {
        (Some(expires_at), Some(_)) => Instant::now() + EXPIRY_BUFFER >= expires_at,
        _ => false,
    };

    if needs_refresh {
        let refresh_token = state.refresh_token.as_deref().unwrap();
        tracing::debug!("access token expired, refreshing");
        let refreshed = state.flow.refresh_token(refresh_token).await?;
        state.access_token = refreshed.access_token;
        state.refresh_token = Some(refreshed.refresh_token);
        state.expires_at = Some(Instant::now() + Duration::from_secs(refreshed.expires_in));
    }

    Ok(state.access_token.clone())
}

async fn app_flow_actor_loop(mut auth: GithubAppAuth, mut rx: mpsc::UnboundedReceiver<Command>) {
    while let Some(Command::GetToken(reply)) = rx.recv().await {
        let result = auth.get_or_refresh().await;
        let _ = reply.send(result);
    }
}

pub struct GithubAuthMiddleware {
    sender: mpsc::UnboundedSender<Command>,
}

impl GithubAuthMiddleware {
    pub fn from_app_flow(token: AccessTokenResponse, client_id: String) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        let expires_at = token
            .expires_in
            .map(|secs| Instant::now() + Duration::from_secs(secs));

        let state = TokenState {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at,
            flow: DeviceAuthFlow::new(client_id),
        };

        tokio::spawn(device_flow_actor_loop(state, receiver));

        Self { sender }
    }

    pub async fn from_github_app(
        app_id: String,
        installation_id: String,
        key: SecretValue,
    ) -> Result<Self> {
        let auth = GithubAppAuth::authenticate(app_id, installation_id, key).await?;
        let (sender, receiver) = mpsc::unbounded_channel();

        tokio::spawn(app_flow_actor_loop(auth, receiver));

        Ok(Self { sender })
    }

    pub fn from_token(token: impl Into<SecretValue>) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let token = token.into();

        tokio::spawn(async move {
            while let Some(Command::GetToken(reply)) = receiver.recv().await {
                let _ = reply.send(Ok(token.as_sensitive_str().to_string()));
            }
        });

        Self { sender }
    }

    async fn get_token(&self) -> Result<String> {
        let (tx, rx) = oneshot::channel();

        self.sender
            .send(Command::GetToken(tx))
            .map_err(|_| anyhow::anyhow!("token actor dropped"))?;

        rx.await
            .map_err(|_| anyhow::anyhow!("token actor dropped"))?
    }
}

#[async_trait::async_trait]
impl Middleware for GithubAuthMiddleware {
    async fn handle(
        &self,
        mut req: reqwest::Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        let token = self.get_token().await.map_err(|e| {
            reqwest_middleware::Error::Middleware(anyhow::anyhow!(
                "failed to get auth token: {}",
                e
            ))
        })?;

        req.headers_mut().insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", token))
                .expect("token contains invalid header characters"),
        );

        next.run(req, extensions).await
    }
}
