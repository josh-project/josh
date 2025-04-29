//! This module implements a do_cgi function, to run CGI scripts with hyper
use futures::TryStreamExt;
use futures::stream::StreamExt;
use hyper::{Request, Response};
use std::process::Stdio;
use std::str::FromStr;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::process::Command;

/// do_cgi is an async function that takes a hyper request and a CGI compatible
/// command, and passes the request to be executed to the command.
/// It then returns a hyper response and the stderr output of the command.
pub async fn do_cgi(
    req: Request<hyper::Body>,
    cmd: Command,
) -> (hyper::http::Response<hyper::Body>, Vec<u8>) {
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

    let req_body = req
        .into_body()
        .map(|result| {
            result.map_err(|_error| std::io::Error::new(std::io::ErrorKind::Other, "Error!"))
        })
        .into_async_read();

    let mut req_body = to_tokio_async_read(req_body);
    let mut err_output = vec![];

    let mut stdout = BufReader::new(stdout);

    let mut data = vec![];
    let write_stdin = async {
        let mut stdin = stdin;
        tokio::io::copy(&mut req_body, &mut stdin).await
    };

    let read_stderr = async {
        let mut stderr = stderr;
        stderr.read_to_end(&mut err_output).await
    };

    let read_stdout = async {
        let mut response = Response::builder();
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
                    hyper::StatusCode::from_u16(
                        u16::from_str(l[1].split(' ').next().unwrap_or("500")).unwrap_or(500),
                    )
                    .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR),
                );
            } else {
                response = response.header(l[0], l[1]);
            }
            line = String::new();
        }
        stdout.read_to_end(&mut data).await?;
        convert_error_io_hyper(response.body(hyper::Body::from(data)))
    };

    let wait_process = async { child.wait().await };

    if let Ok((_, _, response, _)) =
        tokio::try_join!(write_stdin, read_stderr, read_stdout, wait_process)
    {
        return (response, err_output);
    }

    (error_response(), err_output)
}

fn setup_cmd(cmd: &mut Command, req: &Request<hyper::Body>) {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::piped());
    cmd.env("SERVER_SOFTWARE", "hyper")
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
                .get(hyper::header::CONTENT_TYPE)
                .and_then(|x| x.to_str().ok())
                .unwrap_or_default(),
        )
        .env(
            "HTTP_CONTENT_ENCODING",
            req.headers()
                .get(hyper::header::CONTENT_ENCODING)
                .and_then(|x| x.to_str().ok())
                .unwrap_or_default(),
        )
        .env(
            "CONTENT_LENGTH",
            req.headers()
                .get(hyper::header::CONTENT_LENGTH)
                .and_then(|x| x.to_str().ok())
                .unwrap_or_default(),
        );
}

fn to_tokio_async_read(r: impl futures::io::AsyncRead) -> impl tokio::io::AsyncRead {
    tokio_util::compat::FuturesAsyncReadCompatExt::compat(r)
}

fn error_response() -> hyper::Response<hyper::Body> {
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(hyper::Body::empty())
        .unwrap()
}

fn convert_error_io_hyper<T>(res: Result<T, hyper::http::Error>) -> Result<T, std::io::Error> {
    match res {
        Ok(res) => Ok(res),
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "Error!")),
    }
}

#[cfg(test)]
mod tests {
    use hyper::body::HttpBody;

    #[tokio::test]
    async fn run_cmd() {
        let body_content = "a body";

        let req = hyper::Request::builder()
            .method("GET")
            .uri("/some/file?query=aquery")
            .version(hyper::Version::HTTP_11)
            .header("Host", "localhost:8001")
            .header("User-Agent", "test/2.25.1")
            .header("Accept", "*/*")
            .header("Accept-Encoding", "deflate, gzip, br")
            .header("Accept-Language", "en-US, *;q=0.9")
            .header("Pragma", "no-cache")
            .body(hyper::Body::from("\r\na body"))
            .unwrap();

        let mut cmd = tokio::process::Command::new("cat");
        cmd.arg("-");

        let (resp, stderr) = super::do_cgi(req, cmd).await;

        assert_eq!(resp.status(), hyper::StatusCode::OK);

        let resp_string = resp.into_body().data().await.unwrap().unwrap().to_vec();
        let resp_string = String::from_utf8_lossy(&resp_string);

        assert_eq!("", std::str::from_utf8(&stderr).unwrap());
        assert_eq!(body_content, resp_string);
    }
}
