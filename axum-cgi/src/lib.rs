use axum::body::Body;
use futures::TryStreamExt;
use http::{Request, StatusCode};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::io::ReaderStream;

use std::io;
use std::process::Stdio;
use std::str::FromStr;

pub type BodyStream = ReaderStream<BufReader<tokio::process::ChildStdout>>;

async fn copy_request_body_to_stdin(
    req: Request<Body>,
    mut stdin: tokio::process::ChildStdin,
) -> io::Result<()> {
    // Convert axum body to async read
    let data_stream = req.into_body().into_data_stream();
    let stream_of_bytes = TryStreamExt::map_err(data_stream, io::Error::other);
    let async_read = tokio_util::io::StreamReader::new(stream_of_bytes);

    let mut req_body = std::pin::pin!(async_read);

    tokio::io::copy(&mut req_body, &mut stdin).await?;
    stdin.flush().await?;

    Ok(())
}

async fn monitor_process_completion(
    mut child: tokio::process::Child,
    mut stderr: tokio::process::ChildStderr,
) -> io::Result<()> {
    let mut err_output = Vec::new();
    stderr.read_to_end(&mut err_output).await?;

    let status = child.wait().await?;

    if !status.success() {
        let stderr_str = String::from_utf8_lossy(&err_output);
        tracing::error!(
            "CGI process exited with non-zero status: {:?}, stderr: {}",
            status,
            stderr_str
        );
    }

    Ok(())
}

pub async fn do_cgi(
    req: Request<Body>,
    cmd: tokio::process::Command,
) -> io::Result<(http::response::Builder, BodyStream)> {
    let mut cmd = cmd;

    setup_cmd(&mut cmd, &req);
    let mut child = cmd.spawn()?;

    let stdin = match child.stdin.take() {
        Some(i) => i,
        None => {
            return Err(io::Error::other("unable to open stdin"));
        }
    };

    let stdout = match child.stdout.take() {
        Some(o) => o,
        None => {
            return Err(io::Error::other("unable to open stdout"));
        }
    };

    let stderr = match child.stderr.take() {
        Some(e) => e,
        None => {
            return Err(io::Error::other("unable to open stderr"));
        }
    };

    tokio::spawn(async move {
        match copy_request_body_to_stdin(req, stdin).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("Failed to copy request body to CGI stdin: {}", e);
            }
        }
    });

    tokio::spawn(async move {
        match monitor_process_completion(child, stderr).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("Failed to monitor CGI process: {}", e);
            }
        }
    });

    // Parse headers from stdout
    let mut stdout = BufReader::new(stdout);
    let mut response = http::Response::builder();
    let mut line = String::new();

    while stdout.read_line(&mut line).await.unwrap_or(0) > 0 {
        line = line
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_owned();

        let l: Vec<&str> = line.splitn(2, ": ").collect();
        if l.len() < 2 {
            break;
        }

        if l[0] == "Status" {
            response = response.status(
                StatusCode::from_u16(
                    u16::from_str(l[1].split(' ').next().unwrap_or("500")).unwrap_or(500),
                )
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            );
        } else {
            response = response.header(l[0], l[1]);
        }

        line.clear();
    }

    Ok((response, ReaderStream::new(stdout)))
}

fn setup_cmd(cmd: &mut tokio::process::Command, req: &Request<Body>) {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::piped());
    cmd.env("SERVER_SOFTWARE", "axum")
        .env("SERVER_NAME", "localhost") // TODO
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", "HTTP/1.1") // TODO
        .env("SERVER_PORT", "80") // TODO
        .env("REQUEST_METHOD", format!("{}", req.method()))
        .env("SCRIPT_NAME", "") // TODO
        .env("QUERY_STRING", req.uri().query().unwrap_or(""))
        .env("REMOTE_ADDR", "") // TODO
        .env("AUTH_TYPE", "") // TODO
        .env("REMOTE_USER", "") // TODO
        .env(
            "CONTENT_TYPE",
            req.headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|x| x.to_str().ok())
                .unwrap_or_default(),
        )
        .env(
            "HTTP_CONTENT_ENCODING",
            req.headers()
                .get(http::header::CONTENT_ENCODING)
                .and_then(|x| x.to_str().ok())
                .unwrap_or_default(),
        )
        .env(
            "CONTENT_LENGTH",
            req.headers()
                .get(http::header::CONTENT_LENGTH)
                .and_then(|x| x.to_str().ok())
                .unwrap_or_default(),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn run_cmd() {
        let body_content = "a body";

        let req = http::Request::builder()
            .method("GET")
            .uri("/some/file?query=aquery")
            .version(http::Version::HTTP_11)
            .header(http::header::HOST, "localhost:8001")
            .header(http::header::USER_AGENT, "test/2.25.1")
            .header(http::header::ACCEPT, "*/*")
            .header(http::header::ACCEPT_ENCODING, "deflate, gzip, br")
            .header(http::header::ACCEPT_LANGUAGE, "en-US, *;q=0.9")
            .header(http::header::PRAGMA, "no-cache")
            .body(Body::from("\r\na body"))
            .unwrap();

        let mut cmd = tokio::process::Command::new("cat");
        cmd.arg("-");

        let (response_builder, stream) = do_cgi(req, cmd).await.unwrap();
        let resp = response_builder.body(Body::from_stream(stream)).unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let resp_string = String::from_utf8_lossy(&body_bytes);
        assert_eq!(body_content, resp_string);
    }
}
