use base64;
use futures::future;
use futures::FutureExt;
use futures::TryStreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server};
use hyper::header::HeaderValue;
use std::env;
use std::net;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::stream::StreamExt;
use tokio::io::BufReader;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use std::str::FromStr;

#[macro_export]
macro_rules! some_or {
    ($e:expr, $b:block) => {
        if let Some(x) = $e {
            x
        } else {
            $b
        }
    };
}

#[macro_export]
macro_rules! ok_or {
    ($e:expr, $b:block) => {
        if let Ok(x) = $e {
            x
        } else {
            $b
        }
    };
}

pub struct ServeTestGit {
    repo_path: PathBuf,
    username: String,
    password: String,
}

pub fn parse_auth(
    req: &hyper::Request<hyper::Body>,
) -> Option<(String, String)> {
    let line = some_or!(
        req.headers()
            .get("authorization")
            .and_then(|h| Some(h.as_bytes())),
        {
            return None;
        }
    );
    let u = ok_or!(String::from_utf8(line[6..].to_vec()), {
        return None;
    });
    let decoded = ok_or!(base64::decode(&u), {
        return None;
    });
    let s = ok_or!(String::from_utf8(decoded), {
        return None;
    });
    if let [username, password] =
        s.as_str().split(':').collect::<Vec<_>>().as_slice()
    {
        return Some((username.to_string(), password.to_string()));
    }
    return None;
}

fn auth_response(
    req: &Request<hyper::Body>,
    username: &str,
    password: &str,
) -> Option<Response<hyper::Body>> {
    let (rusername, rpassword) = match parse_auth(req) {
        Some(x) => x,
        None => {
            println!("ServeTestGit: no credentials in request");
            let builder = Response::builder()
                .header("WWW-Authenticate", "Basic realm=User Visible Realm")
                .status(hyper::StatusCode::UNAUTHORIZED);
            return Some(
                builder
                    .body(hyper::Body::empty())
                    .unwrap(),
            );
        }
    };

    if rusername != "admin" && (rusername != username || rpassword != password)
    {
        println!("ServeTestGit: wrong user/pass");
        println!("user: {:?} - {:?}", rusername, username);
        println!("pass: {:?} - {:?}", rpassword, password);
        let builder = Response::builder()
            .header("WWW-Authenticate", "")
            .header("Basic realm", "User Visible Realm")
            .status(hyper::StatusCode::UNAUTHORIZED);
        return Some(
            builder
                .body(hyper::Body::empty())
                .unwrap_or(Response::default()),
        );
    }

    println!("CREDENTIALS OK {:?} {:?}", &rusername, &rpassword);
    return None;
}

async fn call(
    serv: Arc<ServeTestGit>,
    req: Request<hyper::Body>,
) -> Response<hyper::Body> {
    println!("call");

    if let Some(response) = auth_response(&req, &serv.username, &serv.password)
    {
        return response;
    }

    let path = &serv.repo_path;

    println!("ServeTestGit CALLING git-http backend");
    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&path);
    cmd.env("GIT_PROJECT_ROOT", &path);
    /* cmd.env("PATH_TRANSLATED", "/"); */
    cmd.env("GIT_DIR", &path);
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.env("PATH_INFO", req.uri().path());

    do_cgi(req, cmd).await
}

async fn run_test_server(
    addr: net::SocketAddr,
    repo_path: &Path,
    username: &str,
    password: &str,
) {
    let serve_test_git = Arc::new(ServeTestGit {
        repo_path: repo_path.to_owned(),
        username: username.to_owned(),
        password: password.to_owned(),
    });

    let make_service = make_service_fn(move |_| {
        let serve_test_git = serve_test_git.clone();

        let service = service_fn(move |_req| {
            let serve_test_git = serve_test_git.clone();

            call(serve_test_git, _req).map(Ok::<_, hyper::http::Error>)
        });

        future::ok::<_, hyper::http::Error>(service)
    });

    let server = Server::bind(&addr).serve(make_service);

    println!("Now listening on {}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

#[tokio::main]
async fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    println!("RUN HTTP SERVER {:?}", args);

    println!("args: {:?}", args);

    let app = clap::App::new("josh-test-server")
        .arg(
            clap::Arg::with_name("local")
                .long("local")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .arg(
            clap::Arg::with_name("password")
                .long("password")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("username")
                .long("username")
                .takes_value(true),
        );

    let args = app.get_matches_from(args);

    let port = args.value_of("port").unwrap_or("8000").to_owned();

    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    run_test_server(
        addr,
        &PathBuf::from(
            args.value_of("local").expect("missing local directory"),
        ),
        args.value_of("username").expect("missing username"),
        args.value_of("password").expect("missing password"),
    ).await;

    ()
}

async fn do_cgi(
    req: Request<hyper::Body>,
    cmd: Command,
) -> hyper::http::Response<hyper::Body> {
    let mut cmd = cmd;
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
            req.headers().get(hyper::header::CONTENT_TYPE).unwrap_or(&HeaderValue::from_static("")).to_str().unwrap()
        )
        .env(
            "HTTP_CONTENT_ENCODING",
            req.headers().get(hyper::header::CONTENT_ENCODING).unwrap_or(&HeaderValue::from_static("")).to_str().unwrap()
        )
        .env(
            "CONTENT_LENGTH",
            req.headers().get(hyper::header::CONTENT_LENGTH).unwrap_or(&HeaderValue::from_static("")).to_str().unwrap()
        );

    println!("{:?}", cmd);

    let mut child = cmd.spawn().expect("can't spawn CGI command");
    let mut stdin = child.stdin.as_mut().expect("Failed to open stdin");
    let mut stdout = child.stdout.as_mut().expect("Failed to open stdout");
    let mut stderr = child.stderr.as_mut().expect("Failed to open stderr");

    let req_body = req
        .into_body()
        .map(|result| {
            result.map_err(|_error| {
                std::io::Error::new(std::io::ErrorKind::Other, "Error!")
            })
        })
        .into_async_read();

    let mut req_body = to_tokio_async_read(req_body);

    let res = tokio::try_join!(
        async {
            tokio::io::copy(&mut req_body, &mut stdin).await?;
            stdin.shutdown().await?;
            println!("shutdown");
            Ok(())
        },
        build_response(&mut stdout, &mut stderr)
    );

    let (_, r2) = res.unwrap();

    r2
}

fn to_tokio_async_read(
    r: impl futures::io::AsyncRead,
) -> impl tokio::io::AsyncRead {
    tokio_util::compat::FuturesAsyncReadCompatExt::compat(r)
}

async fn build_response(
    stdout: &mut &mut tokio::process::ChildStdout,
    stderr: &mut &mut tokio::process::ChildStderr,
) -> Result<Response<hyper::Body>, std::io::Error> {
    let mut response = Response::builder();

    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    while stdout.read_line(&mut line).await.unwrap_or(0) > 0 {
        line = line.trim_end_matches("\n").trim_end_matches("\r").to_owned();
        println!("{}", line);

        let l: Vec<&str> =
            line.splitn(2, ": ").collect();
        if l.len() < 2 {
            break;
        }
        if l[0] == "Status" {
            response = response.status(hyper::StatusCode::from_u16(
                u16::from_str(l[1].split(" ").next().unwrap()).unwrap(),
            ).unwrap());
        } else {
            println!("{:?}", l);
            response = response
                .header(l[0], l[1]);
        }
        line = String::new();
    }

    let mut data = vec![];
    stdout
        .read_to_end(&mut data).await.unwrap();

    let mut stderrdata = vec![];
    stderr
        .read_to_end(&mut stderrdata).await.unwrap();

    println!("STDERRDATA: {:?}", String::from_utf8(stderrdata.clone()));
    println!("DATA: {:?}", String::from_utf8(data.clone()));

    let body = response.body(hyper::Body::from(data));

    println!("BODY: {:?}", body);

    convert_error_io_hyper(body)
}

fn convert_error_io_hyper<T>(res: Result<T, hyper::http::Error>) -> Result<T, std::io::Error>
{
    match res {
        Ok(res) => Ok(res),
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "Error!"))
    }
}
