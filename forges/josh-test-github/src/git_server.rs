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

pub struct GitServer {
    task: tokio::task::JoinHandle<()>,
    port: u16,
}

impl GitServer {
    pub async fn new(repo_path: &Path) -> anyhow::Result<Self> {
        let repo_path = repo_path.canonicalize()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();

        let app = Router::new()
            .route("/{*path}", get(handle_get).post(handle_post))
            .with_state(repo_path.clone());

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
    State(repo_path): State<PathBuf>,
    AxumPath(path): AxumPath<String>,
    req: axum::extract::Request,
) -> Response<Body> {
    serve_git(&repo_path, &path, req).await
}

async fn handle_post(
    State(repo_path): State<PathBuf>,
    AxumPath(path): AxumPath<String>,
    req: axum::extract::Request,
) -> Response<Body> {
    serve_git(&repo_path, &path, req).await
}

async fn serve_git(repo_path: &Path, path: &str, req: axum::extract::Request) -> Response<Body> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("http-backend");

    let repo_dir = repo_path.file_name().unwrap().to_str().unwrap();
    let path_info = format!("/{}/{}", repo_dir, path.trim_start_matches('/'));
    let path_info = path_info.trim_end_matches('/');

    cmd.env("GIT_PROJECT_ROOT", repo_path.parent().unwrap())
        .env("PATH_INFO", path_info)
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
            }
        }
    }

    // Read stderr in background
    tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.ok();
    });

    let stream = tokio_util::io::ReaderStream::new(stdout);
    Response::builder()
        .status(status)
        .body(Body::from_stream(stream))
        .unwrap()
}
