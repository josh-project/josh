use anyhow::Result;
use reqwest::header;
use reqwest_middleware::{Middleware, Next};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

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

async fn token_actor_loop(mut state: TokenState, mut rx: mpsc::Receiver<Command>) {
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

pub struct GithubAuthMiddleware {
    sender: mpsc::Sender<Command>,
}

impl GithubAuthMiddleware {
    pub fn new(token: AccessTokenResponse, client_id: String) -> Self {
        let (sender, receiver) = mpsc::channel(8);

        let expires_at = token
            .expires_in
            .map(|secs| Instant::now() + Duration::from_secs(secs));

        let state = TokenState {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at,
            flow: DeviceAuthFlow::new(client_id),
        };

        tokio::spawn(token_actor_loop(state, receiver));

        Self { sender }
    }

    async fn get_token(&self) -> Result<String> {
        let (tx, rx) = oneshot::channel();

        self.sender
            .send(Command::GetToken(tx))
            .await
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
