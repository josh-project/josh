use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use tokio::sync::mpsc;

use josh_core::cache::{CacheStack, TransactionContext};
use josh_github_graphql::connection::GithubApiConnection;

use crate::fetch::handle_fetch;
use crate::models::CqActorState;
use crate::step::run_queue_cycle;
use crate::track::handle_track;
use crate::types::{CqEvent, GH_TOKEN_ENV, TrackRequest, UserAction};
use crate::webhook::handle_webhook;

async fn track_handler(
    State(event_tx): State<mpsc::Sender<CqEvent>>,
    axum::Json(req): axum::Json<TrackRequest>,
) -> impl IntoResponse {
    enqueue(&event_tx, CqEvent::Track(req)).await
}

async fn webhook_handler(
    State(event_tx): State<mpsc::Sender<CqEvent>>,
    payload: josh_github_webhooks::webhook_server::WebhookPayload,
) -> impl IntoResponse {
    enqueue(&event_tx, CqEvent::Webhook(payload)).await
}

async fn enqueue(event_tx: &mpsc::Sender<CqEvent>, event: CqEvent) -> (StatusCode, &'static str) {
    match event_tx.send(event).await {
        Ok(()) => (StatusCode::ACCEPTED, "accepted"),
        Err(e) => {
            tracing::error!(error = ?e, "failed to enqueue event");
            (StatusCode::SERVICE_UNAVAILABLE, "event queue closed")
        }
    }
}

pub fn make_router(event_tx: mpsc::Sender<CqEvent>) -> axum::Router {
    axum::Router::new()
        .route("/v1/track", post(track_handler))
        .route("/v1/webhook", post(webhook_handler))
        .with_state(event_tx)
}

fn handle_action(action: UserAction) {
    match action {
        UserAction::Message(message) => {
            eprintln!("{}", message)
        }
    }
}

pub fn spawn_serve_task(
    repo_path: PathBuf,
    cache: Arc<CacheStack>,
    tick_interval_secs: u64,
    api: Option<Arc<GithubApiConnection>>,
    // Maps arbitrary clone URLs (e.g. 127.0.0.1 for tests) to (owner, name) pairs.
    url_owner_map: HashMap<String, (String, String)>,
) -> mpsc::Sender<CqEvent> {
    let (event_tx, mut event_rx) = mpsc::channel::<CqEvent>(100);

    let api = api.or_else(|| GithubApiConnection::from_environment().map(Arc::new));

    if api.is_none() {
        tracing::warn!("{} not set and no stored credentials found", GH_TOKEN_ENV);
    }

    // Spawn the periodic tick timer
    let tick_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(tick_interval_secs));
        // Skip the immediate first tick — wait one full interval before
        // the first fetch, giving the server time to start and webhooks
        // to arrive.
        interval.tick().await;
        loop {
            interval.tick().await;
            if tick_tx.send(CqEvent::Tick).await.is_err() {
                break; // channel closed
            }
        }
    });

    // Spawn the actor — serializes all state access
    tokio::task::spawn_blocking(move || {
        let mut state = CqActorState {
            url_owner_map,
            ..Default::default()
        };

        while let Some(event) = event_rx.blocking_recv() {
            let transaction = match TransactionContext::new(&repo_path, cache.clone()).open(None) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to open transaction");
                    continue;
                }
            };

            match event {
                CqEvent::Tick => {
                    tracing::info!("tick: running fetch");
                    state = match handle_fetch(&transaction, api.as_deref(), state.clone()) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(error = ?e, "fetch failed");
                            continue;
                        }
                    };
                }
                CqEvent::Track(req) => {
                    match handle_track(&req.url, &req.id, &req.mode, &transaction) {
                        Ok(action) => handle_action(action),
                        Err(e) => tracing::error!(error = ?e, "track failed"),
                    };
                }
                CqEvent::Webhook(payload) => {
                    state =
                        match handle_webhook(&payload, &transaction, api.as_deref(), state.clone())
                        {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::error!(error = ?e, "webhook handling error");
                                continue;
                            }
                        };
                }
            }

            run_queue_cycle(&mut state, &transaction, api.as_deref());
        }
    });

    event_tx
}
