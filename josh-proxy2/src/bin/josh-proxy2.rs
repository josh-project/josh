use base64;
use futures::future;
use futures::FutureExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server};
use std::env;
use std::net;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;


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
    let line = josh::some_or!(
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
            return Some(builder.body(hyper::Body::empty()).unwrap());
        }
    };

    if rusername != "admin" && (rusername != username || rpassword != password)
    {
        println!("ServeTestGit: wrong user/pass");
        println!("user: {:?} - {:?}", rusername, username);
        println!("pass: {:?} - {:?}", rpassword, password);
        let builder = Response::builder()
            .header("WWW-Authenticate", "Basic realm=User Visible Realm")
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

    hyper_cgi::do_cgi(req, cmd).await.0
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
    )
    .await;

    ()
}
