use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use tokio::sync::{mpsc, oneshot};

use josh_core::cache::{CacheStack, Transaction, TransactionContext};
use josh_github_graphql::connection::GithubApiConnection;

use crate::fetch::handle_fetch;
use crate::models::CqActorState;
use crate::step::run_queue_cycle;
use crate::track::handle_track;
use crate::types::{CqEvent, GH_TOKEN_ENV, TrackRequest};
use crate::webhook::handle_webhook;

async fn track_handler(
    State(event_tx): State<mpsc::Sender<CqEvent>>,
    axum::Json(req): axum::Json<TrackRequest>,
) -> impl IntoResponse {
    let (tx, rx) = tokio::sync::oneshot::channel();
    if event_tx
        .send(CqEvent::Track {
            request: req,
            done: tx,
        })
        .await
        .is_err()
    {
        return (StatusCode::SERVICE_UNAVAILABLE, "event queue closed");
    }
    match rx.await {
        Ok(()) => (StatusCode::OK, "tracked"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "actor dropped"),
    }
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

pub async fn bind_router(
    event_tx: mpsc::Sender<CqEvent>,
) -> anyhow::Result<(tokio::task::JoinHandle<()>, String)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let cq_url = format!("http://127.0.0.1:{}", addr.port());
    let app = make_router(event_tx);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("CQ HTTP server failed");
    });
    Ok((handle, cq_url))
}

/// Open a transaction on the metarepo, logging and returning `None` on failure.
fn open_transaction(repo_path: &Path, cache: &Arc<CacheStack>) -> Option<Transaction> {
    match TransactionContext::new(repo_path, cache.clone()).open(None) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::error!(error = ?e, "failed to open transaction");
            None
        }
    }
}

/// Process a single actor event, returning the completion signal (if the event
/// carried one) so the caller can fire it after the queue cycle runs. The signal
/// is returned rather than fired here so it is always delivered, regardless of
/// which path the event took.
async fn process_event(
    event: CqEvent,
    repo_path: &Path,
    cache: &Arc<CacheStack>,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> Option<oneshot::Sender<()>> {
    match event {
        CqEvent::Tick { done } => {
            tracing::info!("tick: running fetch");
            if let Some(transaction) = open_transaction(repo_path, cache)
                && let Err(e) = handle_fetch(transaction, api, state).await
            {
                tracing::error!(error = ?e, "fetch failed");
            }
            done
        }
        CqEvent::Webhook(payload) => {
            if let Some(transaction) = open_transaction(repo_path, cache)
                && let Err(e) = handle_webhook(&payload, transaction, api, state).await
            {
                tracing::error!(error = ?e, "webhook handling error");
            }
            None
        }
        CqEvent::Track { request, done } => {
            if let Some(transaction) = open_transaction(repo_path, cache) {
                match tokio::task::spawn_blocking(move || {
                    handle_track(&request.url, &request.id, &transaction)
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => tracing::error!(error = ?e, "track failed"),
                    Err(e) => tracing::error!(error = ?e, "track task panicked"),
                }
            }
            Some(done)
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
            if tick_tx.send(CqEvent::Tick { done: None }).await.is_err() {
                break; // channel closed
            }
        }
    });

    // Spawn the actor — serializes all state access
    tokio::spawn(async move {
        let mut state = CqActorState {
            url_owner_map,
            ..Default::default()
        };

        while let Some(event) = event_rx.recv().await {
            // Track adds a remote inline and does not need a merge cycle;
            // Tick and Webhook both fall through to run_queue_cycle.
            let is_track = matches!(event, CqEvent::Track { .. });

            let done = process_event(event, &repo_path, &cache, api.as_deref(), &mut state).await;

            if !is_track {
                run_queue_cycle(&mut state, &repo_path, &cache, api.as_deref()).await;
            }

            if let Some(tx) = done {
                let _ = tx.send(());
            }
        }
    });

    event_tx
}
