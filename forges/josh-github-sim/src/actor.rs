use std::collections::HashMap;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use tokio::sync::{mpsc, oneshot};

pub(crate) enum ActorMsg {
    ServeGitHttp {
        owner: String,
        name: String,
        request: axum::extract::Request,
        response: oneshot::Sender<Response<Body>>,
    },
    GraphQLRequest {
        request: axum::extract::Request,
        response: oneshot::Sender<Response<Body>>,
    },
}

pub(crate) async fn run_actor(
    mut rx: mpsc::UnboundedReceiver<ActorMsg>,
    repos: HashMap<(String, String), PathBuf>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ActorMsg::ServeGitHttp {
                owner,
                name,
                request,
                response,
            } => {
                let key = (owner, name);
                let result = match repos.get(&key) {
                    Some(repo_path) => {
                        josh_cq_test_components::git_http::serve(repo_path, request).await
                    }
                    None => Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("repository not found"))
                        .expect("building error response"),
                };
                if response.send(result).is_err() {
                    tracing::error!("failed to send ServeGitHttp response");
                }
            }
            ActorMsg::GraphQLRequest { request, response } => {
                let result = crate::graphql::handle_graphql_request(&repos, request).await;
                if response.send(result).is_err() {
                    tracing::error!("failed to send GraphQLRequest response");
                }
            }
        }
    }
}
