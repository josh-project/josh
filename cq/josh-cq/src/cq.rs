use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use josh_github_webhooks::webhook_server::WebhookPayload;
use tokio::sync::mpsc;

pub enum CqEvent {
    Webhook(WebhookPayload),
}

async fn webhook_handler(
    State(event_tx): State<mpsc::Sender<CqEvent>>,
    payload: WebhookPayload,
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
        .route("/v1/webhook", post(webhook_handler))
        .with_state(event_tx)
}

pub fn spawn_serve_task() -> mpsc::Sender<CqEvent> {
    let (event_tx, mut event_rx) = mpsc::channel::<CqEvent>(100);

    tokio::task::spawn_blocking(move || {
        while let Some(event) = event_rx.blocking_recv() {
            match event {
                CqEvent::Webhook(payload) => {
                    println!("received webhook: {payload:?}");
                }
            }
        }
    });

    event_tx
}
