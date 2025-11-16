use bytes::Bytes;
use core::iter;
use core::str::from_utf8;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use rand::{Rng, distr::Alphanumeric, rng};
use std::net::SocketAddr;
use std::sync::LazyLock;
use tokio::net::TcpListener;

use futures::FutureExt;
use std::str::FromStr;

use hyper::body::Incoming;

// Import the base64 crate Engine trait anonymously so we can
// call its methods without adding to the namespace.
use base64::engine::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

#[macro_export]
macro_rules! some_or {
    ($e:expr, $b:block) => {
        if let Some(x) = $e { x } else { $b }
    };
}

#[macro_export]
macro_rules! ok_or {
    ($e:expr, $b:block) => {
        if let Ok(x) = $e { x } else { $b }
    };
}

static ARGS: LazyLock<clap::ArgMatches> = LazyLock::new(|| parse_args());

pub struct ServerState {
    users: Vec<(String, String)>,
}

pub fn parse_auth(req: &hyper::Request<Incoming>) -> Option<(String, String)> {
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
    let decoded = ok_or!(BASE64.decode(&u), {
        return None;
    });
    let s = ok_or!(String::from_utf8(decoded), {
        return None;
    });
    if let [username, password] = s.as_str().split(':').collect::<Vec<_>>().as_slice() {
        return Some((username.to_string(), password.to_string()));
    }
    return None;
}

fn auth_response(
    req: &hyper::Request<Incoming>,
    users: &Vec<(String, String)>,
) -> Option<hyper::Response<Full<Bytes>>> {
    if users.len() == 0 {
        return None;
    }

    let (rusername, rpassword) = match parse_auth(req) {
        Some(x) => x,
        None => {
            println!("no credentials in request");
            let builder = hyper::Response::builder()
                .header("WWW-Authenticate", "Basic realm=User Visible Realm")
                .status(hyper::StatusCode::UNAUTHORIZED);
            return Some(builder.body(Full::new(Bytes::new())).unwrap());
        }
    };

    for (username, password) in users {
        if rusername == *username && rpassword == *password {
            println!("CREDENTIALS OK {:?} {:?}", &rusername, &rpassword);
            return None;
        }
    }

    println!("ServerState: wrong user/pass");
    println!("user: {:?}", rusername);
    println!("pass: {:?}", rpassword);
    let builder = hyper::Response::builder()
        .header("WWW-Authenticate", "Basic realm=User Visible Realm")
        .status(hyper::StatusCode::UNAUTHORIZED);
    return Some(
        builder
            .body(Full::new(Bytes::new()))
            .unwrap_or(hyper::Response::default()),
    );
}

async fn call(
    serv: std::sync::Arc<std::sync::Mutex<ServerState>>,
    mut req: hyper::Request<Incoming>,
) -> hyper::Response<Full<Bytes>> {
    println!("call {:?}", req.uri().path());

    let path = req.uri().path();

    if path == "/_noauth" {
        serv.lock().unwrap().users = vec![];
        return hyper::Response::default();
    }

    if path.starts_with("/_make_user/") {
        let builder = hyper::Response::builder();
        let username = path[12..].to_owned();
        let password = iter::repeat(())
            .map(|()| rng().sample(Alphanumeric))
            .take(10)
            .collect::<Vec<u8>>();
        let mut password: String = from_utf8(&password).unwrap().to_string();

        for (u, p) in serv.lock().unwrap().users.iter() {
            if username == *u {
                password = p.clone();
                return builder.body(Full::new(Bytes::from(password))).unwrap();
            }
        }
        serv.lock()
            .unwrap()
            .users
            .push((username, password.clone()));
        println!("users: {:?}", serv.lock().unwrap().users);
        return builder.body(Full::new(Bytes::from(password))).unwrap();
    }

    if let Some(response) = auth_response(&req, &serv.lock().unwrap().users) {
        return response;
    }

    if let Some(proxy) = &ARGS.get_one::<String>("proxy") {
        for proxy in proxy.split(",") {
            if let [proxy_path, proxy_target] = proxy.split("=").collect::<Vec<_>>().as_slice() {
                if let Some(ppath) = path.strip_prefix(proxy_path) {
                    let client_ip = std::net::IpAddr::from_str("127.0.0.1").unwrap();
                    *req.uri_mut() = ppath.parse().unwrap();
                    println!("proxy {:?}", req.uri().path());
                    return match hyper_reverse_proxy::call(client_ip, proxy_target, req).await {
                        Ok(response) => response,
                        Err(error) => hyper::Response::builder()
                            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Full::new(Bytes::from(format!("Proxy error: {:?}", error))))
                            .unwrap(),
                    };
                }
            }
        }
    }

    let workdir = std::path::PathBuf::from(
        ARGS.get_one::<String>("dir")
            .expect("missing working directory"),
    );

    let mut cmd = tokio::process::Command::new(ARGS.get_one::<String>("cmd").expect("missing cmd"));

    for arg in ARGS.get_many::<String>("args").unwrap() {
        cmd.arg(&arg);
    }
    cmd.current_dir(&workdir);
    cmd.env("PATH_INFO", path);

    hyper_cgi::do_cgi(req, cmd).await.0
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server_state = std::sync::Arc::new(std::sync::Mutex::new(ServerState { users: vec![] }));

    let addr: SocketAddr = format!(
        "0.0.0.0:{}",
        ARGS.get_one::<String>("port")
            .unwrap_or(&"8000".to_owned())
            .to_owned()
    )
    .parse()
    .unwrap();

    let listener = TcpListener::bind(addr).await?;
    println!("Now listening on {}", addr);
    let server_state = server_state.clone();

    loop {
        let (tcp, _) = listener.accept().await?;
        let io = TokioIo::new(tcp);
        let server_state = server_state.clone();

        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .timer(TokioTimer::new())
                .serve_connection(
                    io,
                    service_fn(move |_req| {
                        let server_state = server_state.clone();

                        call(server_state, _req).map(Ok::<_, hyper::http::Error>)
                    }),
                )
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

fn parse_args() -> clap::ArgMatches {
    let args = {
        let mut args = vec![];
        for arg in std::env::args() {
            args.push(arg);
        }
        args
    };

    println!("ARGS {:?}", args);

    println!("args: {:?}", args);

    let app = clap::Command::new("hyper-cgi-test-server")
        .arg(clap::Arg::new("dir").long("dir"))
        .arg(clap::Arg::new("cmd").long("cmd"))
        .arg(clap::Arg::new("proxy").long("proxy"))
        .arg(clap::Arg::new("args").long("args").short('a').num_args(1..))
        .arg(clap::Arg::new("port").long("port"));

    app.get_matches_from(args)
}
