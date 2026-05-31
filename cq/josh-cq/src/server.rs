use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use tokio::sync::{mpsc, oneshot};

use url::Url;

use josh_github_auth::middleware::GithubAuthMiddleware;
use josh_github_graphql::connection::GithubApiConnection;

use crate::git::{GitActor, GitActorMessage};
use crate::models::CqActorState;
use crate::refresh_remotes::refresh_remotes;
use crate::step::run_queue_cycle;
use crate::types::{CqEvent, TrackRequest};
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

/// Outcome of processing a single actor event.
struct EventOutcome {
    /// Whether the queue cycle (evaluate→step) should run after this event.
    /// Track adds a remote inline and needs no merge; Tick and Webhook do.
    run_queue_cycle: bool,
    /// Completion signal the event carried, fired after the queue cycle runs so
    /// it is always delivered regardless of which path the event took.
    done: Option<oneshot::Sender<()>>,
}

/// Process a single actor event, mutating `state` and reporting whether a queue
/// cycle is warranted plus any completion signal to fire afterwards. All git
/// work goes through the `git` actor.
async fn process_event(
    event: CqEvent,
    git: &GitActor,
    api: &GithubApiConnection,
    state: &mut CqActorState,
) -> EventOutcome {
    match event {
        CqEvent::Tick { done } => {
            tracing::info!("tick: refreshing remotes");

            if let Err(e) = refresh_remotes(git, api, state).await {
                tracing::error!(error = ?e, "remote refresh failed");
            }

            EventOutcome {
                run_queue_cycle: true,
                done,
            }
        }
        CqEvent::Webhook(payload) => {
            if let Err(e) = handle_webhook(&payload, git, api, state).await {
                tracing::error!(error = ?e, "webhook handling error");
            }

            EventOutcome {
                run_queue_cycle: true,
                done: None,
            }
        }
        CqEvent::Track { request, done } => {
            // Fetch the remote so its FETCH_HEAD resolves, then import it.
            if let Err(e) = git
                .request(|reply| GitActorMessage::RunGitCommand {
                    args: vec!["fetch".to_string(), request.url.clone(), "HEAD".to_string()],
                    reply,
                })
                .await
            {
                tracing::error!(error = ?e, "track fetch failed");
                return EventOutcome {
                    run_queue_cycle: false,
                    done: Some(done),
                };
            };

            if let Err(e) = git
                .request(|reply| GitActorMessage::Track {
                    url: request.url,
                    id: request.id,
                    reply,
                })
                .await
            {
                tracing::error!(error = ?e, "track failed");
                return EventOutcome {
                    run_queue_cycle: false,
                    done: Some(done),
                };
            }

            EventOutcome {
                run_queue_cycle: false,
                done: Some(done),
            }
        }
    }
}

pub fn spawn_serve_task(
    tick_interval_secs: u64,
    // Shared git actor handle (created by the caller, which also derives the
    // command stack from the same middleware).
    git: Arc<GitActor>,
    // Auth middleware for the GraphQL connection.
    middleware: Arc<GithubAuthMiddleware>,
    // GraphQL endpoint override; `None` uses the real GitHub API,
    // tests pass the mock's URL.
    api_url: Option<Url>,
    // Maps arbitrary clone URLs (e.g. 127.0.0.1 for tests) to (owner, name) pairs.
    url_owner_map: HashMap<String, (String, String)>,
) -> mpsc::Sender<CqEvent> {
    let (event_tx, mut event_rx) = mpsc::channel::<CqEvent>(100);

    let api = GithubApiConnection::from_middleware(middleware, api_url);

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
            let outcome = process_event(event, &git, &api, &mut state).await;

            if outcome.run_queue_cycle {
                run_queue_cycle(&mut state, &git, &api).await;
            }

            if let Some(tx) = outcome.done {
                let _ = tx.send(());
            }
        }
    });

    event_tx
}
