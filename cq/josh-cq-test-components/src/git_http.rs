use std::path::Path;
use std::process::Stdio;

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use headers::HeaderMapExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

const GIT_CMD: &str = "git";
const GIT_HTTP_BACKEND: &str = "http-backend";

const CGI_GIT_PROJECT_ROOT: &str = "GIT_PROJECT_ROOT";
const CGI_PATH_INFO: &str = "PATH_INFO";
const CGI_GIT_HTTP_EXPORT_ALL: &str = "GIT_HTTP_EXPORT_ALL";
const CGI_REQUEST_METHOD: &str = "REQUEST_METHOD";
const CGI_QUERY_STRING: &str = "QUERY_STRING";
const CGI_CONTENT_TYPE: &str = "CONTENT_TYPE";
const CGI_CONTENT_LENGTH: &str = "CONTENT_LENGTH";

const CGI_STATUS: &str = "Status";
const CGI_HEADER_CONTENT_TYPE: &str = "Content-Type";
const CGI_HEADER_DELIM: &[u8] = b"\r\n\r\n";

fn prepare_command(
    repo_path: &Path,
    method: &str,
    query_string: &str,
    content_type: Option<headers::ContentType>,
    content_length: Option<headers::ContentLength>,
    req_path: &str,
) -> tokio::process::Command {
    let repo_dir = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");

    let path_info = format!("/{}/{}", repo_dir, req_path);
    let path_info = path_info.trim_end_matches('/');

    let ct = content_type
        .as_ref()
        .map(|ct| ct.to_string())
        .unwrap_or_default();
    let cl = content_length
        .map(|cl| cl.0.to_string())
        .unwrap_or_default();

    let mut cmd = tokio::process::Command::new(GIT_CMD);
    cmd.arg(GIT_HTTP_BACKEND)
        .env(
            CGI_GIT_PROJECT_ROOT,
            repo_path.parent().expect("repo path has no parent"),
        )
        .env(CGI_PATH_INFO, path_info)
        .env(CGI_GIT_HTTP_EXPORT_ALL, "1")
        .env(CGI_REQUEST_METHOD, method)
        .env(CGI_QUERY_STRING, query_string)
        .env(CGI_CONTENT_TYPE, &ct)
        .env(CGI_CONTENT_LENGTH, &cl)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());

    cmd
}

pub async fn serve(repo_path: &Path, req: axum::extract::Request) -> Response<Body> {
    let (parts, body) = req.into_parts();

    let method = parts.method.to_string();
    let query_string = parts.uri.query().unwrap_or("").to_string();
    let req_path = parts.uri.path().trim_start_matches('/').to_string();
    let content_type = parts.headers.typed_get::<headers::ContentType>();
    let content_length = parts.headers.typed_get::<headers::ContentLength>();

    let mut child = match prepare_command(
        repo_path,
        &method,
        &query_string,
        content_type,
        content_length,
        &req_path,
    )
    .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!(
                    "failed to spawn git http-backend: {}",
                    e
                )))
                .expect("building error response");
        }
    };

    // Rebuild request with original body for streaming into stdin
    let req = axum::extract::Request::from_parts(parts, body);

    let mut stdin = child
        .stdin
        .take()
        .expect("stdin pipe not captured on git http-backend");
    let mut stdout = child
        .stdout
        .take()
        .expect("stdout pipe not captured on git http-backend");
    let mut stderr = child
        .stderr
        .take()
        .expect("stderr pipe not captured on git http-backend");

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
        drop(stdin);
    }

    // Parse CGI headers from stdout (delimited by \r\n\r\n)
    let mut header_buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        if stdout.read_exact(&mut byte).await.is_err() {
            break;
        }
        header_buf.push(byte[0]);
        if header_buf.ends_with(CGI_HEADER_DELIM) {
            break;
        }
    }

    let headers_str = String::from_utf8_lossy(&header_buf);
    let mut status = StatusCode::OK;
    let mut content_type_out: Option<String> = None;

    for line in headers_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(": ") {
            if name.eq_ignore_ascii_case(CGI_STATUS) {
                status = value
                    .split(' ')
                    .next()
                    .and_then(|s| s.parse::<u16>().ok())
                    .and_then(|code| StatusCode::from_u16(code).ok())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            } else if name.eq_ignore_ascii_case(CGI_HEADER_CONTENT_TYPE) {
                content_type_out = Some(value.to_string());
            }
        }
    }

    // Read remaining stdout (git http-backend exits after writing)
    let mut body_buf = Vec::new();
    stdout.read_to_end(&mut body_buf).await.ok();

    let _ = child.wait().await;

    // Ensure stderr drain completes
    tokio::spawn(async move {
        let _ = stderr_handle.await;
    });

    let mut response = Response::builder().status(status);
    if let Some(ct) = content_type_out {
        response = response.header(CGI_HEADER_CONTENT_TYPE, ct);
    }

    response
        .body(Body::from(body_buf))
        .expect("building CGI response")
}
