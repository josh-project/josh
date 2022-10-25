#[macro_use]
extern crate lazy_static;
use core::iter;
use core::str::from_utf8;
use rand::{distributions::Alphanumeric, thread_rng, Rng};

use futures::FutureExt;

use hyper::server::Server;

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

lazy_static! {
    static ref ARGS: clap::ArgMatches = parse_args();
}

pub struct ServerState {
    users: Vec<(String, String)>,
}

pub fn parse_auth(req: &hyper::Request<hyper::Body>) -> Option<(String, String)> {
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
    if let [username, password] = s.as_str().split(':').collect::<Vec<_>>().as_slice() {
        return Some((username.to_string(), password.to_string()));
    }
    return None;
}

fn auth_response(
    req: &hyper::Request<hyper::Body>,
    users: &Vec<(String, String)>,
) -> Option<hyper::Response<hyper::Body>> {
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
            return Some(builder.body(hyper::Body::empty()).unwrap());
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
            .body(hyper::Body::empty())
            .unwrap_or(hyper::Response::default()),
    );
}

async fn call(
    serv: std::sync::Arc<std::sync::Mutex<ServerState>>,
    req: hyper::Request<hyper::Body>,
) -> hyper::Response<hyper::Body> {
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
            .map(|()| thread_rng().sample(Alphanumeric))
            .take(10)
            .collect::<Vec<u8>>();
        let mut password: String = from_utf8(&password).unwrap().to_string();

        for (u, p) in serv.lock().unwrap().users.iter() {
            if username == *u {
                password = p.clone();
                return builder.body(hyper::Body::from(password)).unwrap();
            }
        }
        serv.lock()
            .unwrap()
            .users
            .push((username, password.clone()));
        println!("users: {:?}", serv.lock().unwrap().users);
        return builder.body(hyper::Body::from(password)).unwrap();
    }

    if let Some(response) = auth_response(&req, &serv.lock().unwrap().users) {
        return response;
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
async fn main() {
    let server_state = std::sync::Arc::new(std::sync::Mutex::new(ServerState { users: vec![] }));

    let make_service = hyper::service::make_service_fn(move |_| {
        let server_state = server_state.clone();

        let service = hyper::service::service_fn(move |_req| {
            let server_state = server_state.clone();

            call(server_state, _req).map(Ok::<_, hyper::http::Error>)
        });

        futures::future::ok::<_, hyper::http::Error>(service)
    });

    let addr = format!(
        "0.0.0.0:{}",
        ARGS.get_one::<String>("port")
            .unwrap_or(&"8000".to_owned())
            .to_owned()
    )
    .parse()
    .unwrap();
    let server = Server::bind(&addr).serve(make_service);
    println!("Now listening on {}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
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
        .arg(clap::Arg::new("args").long("args").short('a').num_args(1..))
        .arg(clap::Arg::new("port").long("port"));

    app.get_matches_from(args)
}
