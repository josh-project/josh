use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::response::Response;
use axum::routing::get;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use url::Url;

#[derive(Clone)]
struct GitServerState {
    repo_path: PathBuf,
    extra_env: HashMap<String, String>,
}

pub struct GitServer {
    task: tokio::task::JoinHandle<()>,
    port: u16,
}

impl GitServer {
    pub async fn new(repo_path: &Path, extra_env: HashMap<String, String>) -> anyhow::Result<Self> {
        let repo_path = repo_path.canonicalize()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();

        let state = GitServerState {
            repo_path: repo_path.clone(),
            extra_env,
        };

        let app = Router::new()
            .route("/{*path}", get(handle_get).post(handle_post))
            .with_state(state);

        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Ok(Self { task, port })
    }

    pub fn url(&self) -> Url {
        Url::parse(&format!("http://127.0.0.1:{}/", self.port)).unwrap()
    }
}

impl Drop for GitServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn handle_get(
    State(state): State<GitServerState>,
    AxumPath(path): AxumPath<String>,
    req: axum::extract::Request,
) -> Response<Body> {
    serve_git(&state, &path, req).await
}

async fn handle_post(
    State(state): State<GitServerState>,
    AxumPath(path): AxumPath<String>,
    req: axum::extract::Request,
) -> Response<Body> {
    serve_git(&state, &path, req).await
}

async fn serve_git(
    state: &GitServerState,
    path: &str,
    req: axum::extract::Request,
) -> Response<Body> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("http-backend");

    let repo_dir = state.repo_path.file_name().unwrap().to_str().unwrap();
    let path_info = format!("/{}/{}", repo_dir, path.trim_start_matches('/'));
    let path_info = path_info.trim_end_matches('/');

    cmd.env("GIT_PROJECT_ROOT", state.repo_path.parent().unwrap())
        .env("PATH_INFO", &path_info)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REQUEST_METHOD", req.method().to_string())
        .env("QUERY_STRING", req.uri().query().unwrap_or(""))
        .env(
            "CONTENT_TYPE",
            req.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
        )
        .env(
            "CONTENT_LENGTH",
            req.headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
        );

    for (key, value) in &state.extra_env {
        cmd.env(key, value);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Response::builder()
                .status(500)
                .body(Body::from(format!("spawn failed: {}", e)))
                .unwrap();
        }
    };

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    // Drain stderr in background immediately to prevent pipe buffer deadlock
    let stderr_handle = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    });

    // Pipe request body to stdin
    {
        use futures::StreamExt;
        let mut body_data = req.into_body().into_data_stream();
        while let Some(chunk) = body_data.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break,
            };
            if stdin.write_all(&chunk).await.is_err() {
                break;
            }
        }
    }
    drop(stdin);

    // Parse CGI headers from stdout
    let mut header_buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        if stdout.read_exact(&mut byte).await.is_err() {
            break;
        }
        header_buf.push(byte[0]);
        let len = header_buf.len();
        if len >= 4
            && header_buf[len - 4] == b'\r'
            && header_buf[len - 3] == b'\n'
            && header_buf[len - 2] == b'\r'
            && header_buf[len - 1] == b'\n'
        {
            break;
        }
    }

    let headers_str = String::from_utf8_lossy(&header_buf);
    let mut status = 200;
    let mut content_type: Option<String> = None;

    for line in headers_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(": ") {
            if name.eq_ignore_ascii_case("Status") {
                status = value
                    .split(' ')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(500);
            } else if name.eq_ignore_ascii_case("Content-Type") {
                content_type = Some(value.to_string());
            }
        }
    }

    // Read all stdout data (git http-backend exits after writing)
    let mut body_buf = Vec::new();
    stdout.read_to_end(&mut body_buf).await.ok();

    // Wait for child to exit
    let _ = child.wait().await;

    // Spawn a task to collect stderr
    tokio::spawn(async move {
        let _ = stderr_handle.await;
    });

    let mut response = Response::builder().status(status);
    if let Some(ct) = content_type {
        response = response.header("Content-Type", ct);
    }
    response.body(Body::from(body_buf)).unwrap()
}
