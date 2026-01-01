use axum::body::Body;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::TryStreamExt;
use http::{Request, StatusCode};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use std::io;
use std::process::Stdio;
use std::str::FromStr;

pub async fn do_cgi(req: Request<Body>, cmd: tokio::process::Command) -> (Response, Vec<u8>) {
    let mut cmd = cmd;
    setup_cmd(&mut cmd, &req);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_e) => {
            return (
                error_response(),
                std::vec::Vec::from("Unable to spawn child command"),
            );
        }
    };

    let stdin = match child.stdin.take() {
        Some(i) => i,
        None => {
            return (
                error_response(),
                std::vec::Vec::from("Unable to open stdin"),
            );
        }
    };

    let stdout = match child.stdout.take() {
        Some(o) => o,
        None => {
            return (
                error_response(),
                std::vec::Vec::from("Unable to open stdout"),
            );
        }
    };

    let stderr = match child.stderr.take() {
        Some(e) => e,
        None => {
            return (
                error_response(),
                std::vec::Vec::from("Unable to open stderr"),
            );
        }
    };

    // Convert axum body to async read
    let data_stream = req.into_body().into_data_stream();
    let stream_of_bytes = TryStreamExt::map_err(data_stream, io::Error::other);
    let async_read = tokio_util::io::StreamReader::new(stream_of_bytes);
    let mut req_body = std::pin::pin!(async_read);

    let mut err_output = vec![];
    let mut stdout = BufReader::new(stdout);
    let mut data = vec![];

    let write_stdin = async {
        let mut stdin = stdin;
        let res = tokio::io::copy(&mut req_body, &mut stdin).await;
        stdin.flush().await.unwrap();
        res
    };

    let read_stderr = async {
        let mut stderr = stderr;
        stderr.read_to_end(&mut err_output).await
    };

    let read_stdout = async {
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
            line = String::new();
        }

        stdout.read_to_end(&mut data).await?;
        convert_error_io_http(response.body(Body::from(Bytes::from(data))))
    };

    let wait_process = async { child.wait().await };

    if let Ok((_, _, response, _)) =
        tokio::try_join!(write_stdin, read_stderr, read_stdout, wait_process)
    {
        return (response.into_response(), err_output);
    }

    (error_response(), err_output)
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

fn error_response() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Body::new(String::new())).into_response()
}

fn convert_error_io_http<T>(res: Result<T, http::Error>) -> Result<T, std::io::Error> {
    match res {
        Ok(res) => Ok(res),
        Err(_) => Err(std::io::Error::other("Error!")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
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
            .body(Body::from(Bytes::from("\r\na body")))
            .unwrap();

        let mut cmd = tokio::process::Command::new("cat");
        cmd.arg("-");

        let (resp, stderr) = do_cgi(req, cmd).await;

        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let resp_string = String::from_utf8_lossy(&body_bytes);

        assert_eq!("", std::str::from_utf8(&stderr).unwrap());
        assert_eq!(body_content, resp_string);
    }
}
