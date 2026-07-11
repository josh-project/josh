use std::path::Path;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

const GIT_CMD: &str = "git";
const GIT_HTTP_BACKEND: &str = "http-backend";

const CGI_GIT_PROJECT_ROOT: &str = "GIT_PROJECT_ROOT";
const CGI_PATH_INFO: &str = "PATH_INFO";
const CGI_GIT_HTTP_EXPORT_ALL: &str = "GIT_HTTP_EXPORT_ALL";

fn prepare_command(repo_path: &Path, req_path: &str) -> tokio::process::Command {
    let repo_dir = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");

    let path_info = format!("/{}/{}", repo_dir, req_path);
    let path_info = path_info.trim_end_matches('/');

    let mut cmd = tokio::process::Command::new(GIT_CMD);
    cmd.arg(GIT_HTTP_BACKEND)
        .env(
            CGI_GIT_PROJECT_ROOT,
            repo_path.parent().expect("repo path has no parent"),
        )
        .env(CGI_PATH_INFO, path_info)
        .env(CGI_GIT_HTTP_EXPORT_ALL, "1");

    cmd
}

pub async fn serve(repo_path: &Path, req: axum::extract::Request) -> Response {
    let req_path = req.uri().path().trim_start_matches('/').to_string();
    let cmd = prepare_command(repo_path, &req_path);

    match axum_cgi::do_cgi(req, cmd).await {
        Ok((response_builder, stream)) => response_builder
            .body(axum::body::Body::from_stream(stream))
            .unwrap_or_else(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to build CGI response",
                )
                    .into_response()
            }),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("CGI error: {}", e),
        )
            .into_response(),
    }
}
