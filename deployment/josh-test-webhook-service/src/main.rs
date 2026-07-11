use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};

use axum::extract;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use secret_vault_value::SecretValue;

use josh_github_webhooks::webhook_signature::{
    GithubWebhookSignature, verify_github_signature_middleware,
};
use josh_test_webhook_service::WebhookPayload;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "WEBHOOK_SECRET")]
    webhook_secret_env: String,

    #[arg(long, default_value = "SOCKET_SECRET")]
    socket_secret_env: String,
}

struct AppState {
    events: std::sync::Mutex<tokio::sync::broadcast::Sender<WebhookPayload>>,
    github_secret: SecretValue,
    socket_secret: SecretValue,
}

impl GithubWebhookSignature for AppState {
    fn webhook_secret(&self) -> SecretValue {
        self.github_secret.clone()
    }
}

async fn websocket_handler(
    axum_auth::AuthBearer(bearer_token): axum_auth::AuthBearer,
    ws: extract::WebSocketUpgrade,
    extract::ConnectInfo(client_addr): extract::ConnectInfo<SocketAddr>,
    extract::State(state): extract::State<Arc<AppState>>,
) -> axum::response::Response {
    if !constant_time_eq::constant_time_eq(
        bearer_token.as_bytes(),
        state.socket_secret.as_sensitive_bytes(),
    ) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, client_addr, state))
}

async fn handle_socket(
    socket: extract::ws::WebSocket,
    client_addr: SocketAddr,
    state: Arc<AppState>,
) {
    use axum::extract::ws::Message;

    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut webhook_rx = state
        .events
        .lock()
        .expect("failed to lock events")
        .subscribe();
    let (ws_message_tx, mut ws_message_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    tracing::info!(?client_addr, "client connected");

    // Spawn task to forward webhooks to WebSocket
    let ws_message_tx_forward = ws_message_tx.clone();
    let forward_task = tokio::spawn(async move {
        'receiving_events: while let Ok(webhook) = webhook_rx.recv().await {
            let message = match bincode::encode_to_vec(webhook, bincode::config::standard()) {
                Ok(message) => Message::Binary(message.into()),
                Err(e) => {
                    tracing::error!(error = ?e, "failed to encode");
                    break 'receiving_events;
                }
            };

            if let Err(e) = ws_message_tx_forward.send(message) {
                tracing::error!(error = ?e, "failed to send message");
                break 'receiving_events;
            }
        }

        tracing::info!(?client_addr, "forward task terminated");
    });

    let reply_task = tokio::spawn(async move {
        'receiving_events: while let Some(msg) = ws_rx.next().await {
            let response = match msg {
                Ok(Message::Ping(data)) => Message::Pong(data),
                Ok(Message::Close(_)) | Err(_) => break 'receiving_events,
                _ => continue 'receiving_events,
            };

            if let Err(e) = ws_message_tx.send(response) {
                tracing::error!(error = ?e, "failed to send response");
                break 'receiving_events;
            }
        }

        tracing::info!(?client_addr, "reply_task terminated");
    });

    while let Some(message) = ws_message_rx.recv().await {
        if let Err(e) = ws_tx.send(message).await {
            tracing::error!(error = ?e, "failed to send to ws");
            break;
        }
    }

    forward_task.abort();
    reply_task.abort();

    tracing::info!(?client_addr, "client disconnected");
}

static ALLOWED_HEADERS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    [
        "x-github-event",
        "x-github-delivery",
        "x-hub-signature",
        "x-hub-signature-256",
        "content-type",
        "user-agent",
        "x-github-hook-id",
        "x-github-hook-installation-target-id",
        "x-github-hook-installation-target-type",
    ]
    .iter()
    .fold(HashSet::new(), |mut acc, header| {
        acc.insert(header.to_string());
        acc
    })
});

async fn webhook_handler(
    headers: axum::http::HeaderMap,
    extract::State(state): extract::State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let filtered_headers = headers
        .into_iter()
        .filter_map(|(header, value)| header.map(|header| (header, value)))
        .filter_map(|(header, value)| {
            if ALLOWED_HEADERS.contains(header.as_str()) {
                match value.to_str() {
                    Ok(value) => Some((header.to_string(), value.to_string())),
                    Err(_) => None,
                }
            } else {
                None
            }
        })
        .collect();

    let payload = WebhookPayload {
        body: body.to_vec(),
        headers: filtered_headers,
    };

    if state.events.lock().unwrap().send(payload).is_err() {
        tracing::info!("no active receivers")
    }

    axum::http::StatusCode::NO_CONTENT.into_response()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use axum::routing::{get, post};

    tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let github_secret = SecretValue::from(std::env::var(&args.webhook_secret_env)?);
    let socket_secret = SecretValue::from(std::env::var(&args.socket_secret_env)?);

    let (bcast_tx, _) = tokio::sync::broadcast::channel(1000);
    let state = Arc::new(AppState {
        events: std::sync::Mutex::new(bcast_tx),
        github_secret,
        socket_secret,
    });

    let signature_layer = {
        let state = state.clone() as Arc<dyn GithubWebhookSignature>;

        axum::middleware::from_fn_with_state(state, verify_github_signature_middleware)
    };

    async fn health() -> axum::response::Response {
        StatusCode::OK.into_response()
    }

    let app = axum::Router::new()
        .route("/ws", get(websocket_handler))
        .route("/webhook", post(webhook_handler).layer(signature_layer))
        .with_state(state)
        .route("/health", get(health))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
