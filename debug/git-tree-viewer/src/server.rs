use std::sync::{mpsc::Sender, Arc};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::Trace;

#[derive(Deserialize)]
struct TraceRequest {
    session: String,
    commit: String,
    label: String,
}

async fn handle_trace(
    State(tx): State<Arc<Sender<Trace>>>,
    Json(req): Json<TraceRequest>,
) -> impl IntoResponse {
    let trace = Trace {
        session: req.session,
        commit: req.commit,
        label: req.label,
    };
    let _ = tx.send(trace);
    StatusCode::ACCEPTED
}

pub fn start(tx: Sender<Trace>) {
    let tx = Arc::new(tx);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async {
            let app = Router::new()
                .route("/v1/trace", post(handle_trace))
                .with_state(tx);

            let listener = tokio::net::TcpListener::bind("127.0.0.1:8765")
                .await
                .expect("Failed to bind HTTP server to 127.0.0.1:8765");

            axum::serve(listener, app).await.expect("HTTP server error");
        });
    });
}
