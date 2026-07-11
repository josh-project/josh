use std::sync::{mpsc::Sender, Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::Trace;

#[derive(Clone)]
struct ServerState {
    tx: Arc<Sender<Trace>>,
    traces: Arc<Mutex<Vec<Trace>>>,
    repo_path: Arc<std::path::Path>,
}

async fn post_trace(
    State(state): State<ServerState>,
    Json(trace): Json<Trace>,
) -> impl IntoResponse {
    state.traces.lock().unwrap().push(trace.clone());
    let _ = state.tx.send(trace);
    StatusCode::ACCEPTED
}

async fn get_traces(State(state): State<ServerState>) -> impl IntoResponse {
    let traces = state.traces.lock().unwrap().clone();
    Json(traces)
}

#[derive(Serialize)]
struct RepoResponse {
    path: String,
}

async fn get_repo(State(state): State<ServerState>) -> impl IntoResponse {
    Json(RepoResponse {
        path: state.repo_path.to_string_lossy().into_owned(),
    })
}

/// Readiness probe. `git-tree-trace` queries this on first use to decide
/// whether a viewer is actually listening; if it can't reach this endpoint it
/// disables tracing (so it never pushes to a dead port).
async fn get_ready() -> impl IntoResponse {
    StatusCode::OK
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
        traces: Arc::new(Mutex::new(Vec::new())),
        repo_path: Arc::from(repo_path),
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        rt.block_on(async {
            let app = Router::new()
                .route("/v1/traces", post(post_trace).get(get_traces))
                .route("/v1/repo", get(get_repo))
                .route("/v1/ready", get(get_ready))
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
