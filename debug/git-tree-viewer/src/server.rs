use std::sync::{mpsc::Sender, Arc};

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::Trace;

#[derive(Deserialize)]
struct TraceRequest {
    session: String,
    commit: String,
    label: String,
}

#[derive(Clone)]
struct ServerState {
    tx: Arc<Sender<Trace>>,
    repo_path: Arc<std::path::Path>,
}

async fn handle_trace(
    State(state): State<ServerState>,
    Json(req): Json<TraceRequest>,
) -> impl IntoResponse {
    let trace = Trace {
        session: req.session,
        commit: req.commit,
        label: req.label,
    };
    let _ = state.tx.send(trace);
    StatusCode::ACCEPTED
}

async fn handle_git(
    State(state): State<ServerState>,
    req: axum::extract::Request,
) -> Response<Body> {
    josh_cq_test_components::git_http::serve(&state.repo_path, req).await
}

const DEFAULT_PORT: u16 = 8765;

pub fn start(tx: Sender<Trace>, repo_path: &std::path::Path) {
    let state = ServerState {
        tx: Arc::new(tx),
        repo_path: Arc::from(repo_path),
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        rt.block_on(async {
            let app = Router::new()
                .route("/v1/trace", post(handle_trace))
                .fallback(get(handle_git).post(handle_git))
                .with_state(state);

            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("Failed to bind HTTP server");

            axum::serve(listener, app).await.expect("HTTP server error");
        });
    });
}
