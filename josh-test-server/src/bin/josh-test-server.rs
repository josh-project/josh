#![allow(clippy::needless_return)]

use futures::future::Future;
use futures::Stream;
use hyper::header::ContentEncoding;
use hyper::header::ContentLength;
use hyper::header::ContentType;
use hyper::header::{Authorization, Basic};
use hyper::server::Http;
use hyper::server::{Request, Response, Service};
use std::env;
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::net;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use tokio_process::CommandExt;

pub struct ServeTestGit {
    handle: tokio_core::reactor::Handle,
    repo_path: PathBuf,
    username: String,
    password: String,
}

fn auth_response(
    req: &Request,
    username: &str,
    password: &str,
) -> Option<Response> {
    let (rusername, rpassword) = match req.headers().get() {
        Some(&Authorization(Basic {
            ref username,
            ref password,
        })) => (
            username.to_owned(),
            password.to_owned().unwrap_or_else(|| "".to_owned()),
        ),
        _ => {
            println!("ServeTestGit: no credentials in request");
            let mut response =
                Response::new().with_status(hyper::StatusCode::Unauthorized);
            response.headers_mut().set_raw(
                "WWW-Authenticate",
                "Basic realm=\"User Visible Realm\"",
            );
            return Some(response);
        }
    };

    if rusername != "admin" && (rusername != username || rpassword != password)
    {
        println!("ServeTestGit: wrong user/pass");
        println!("user: {:?} - {:?}", rusername, username);
        println!("pass: {:?} - {:?}", rpassword, password);
        let mut response =
            Response::new().with_status(hyper::StatusCode::Unauthorized);
        response
            .headers_mut()
            .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\"");
        return Some(response);
    }

    println!("CREDENTIALS OK {:?} {:?}", &rusername, &rpassword);
    return None;
}

impl Service for ServeTestGit {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        println!("call");

        if let Some(response) =
            auth_response(&req, &self.username, &self.password)
        {
            return Box::new(futures::future::ok(response));
        }

        let path = &self.repo_path;

        let handle = self.handle.clone();

        println!("ServeTestGit CALLING git-http backend");
        let mut cmd = Command::new("git");
        cmd.arg("http-backend");
        cmd.current_dir(&path);
        cmd.env("GIT_PROJECT_ROOT", &path);
        /* cmd.env("PATH_TRANSLATED", "/"); */
        cmd.env("GIT_DIR", &path);
        cmd.env("GIT_HTTP_EXPORT_ALL", "");
        cmd.env("PATH_INFO", req.uri().path());

        do_cgi(req, cmd, handle.clone())
    }
}

fn run_test_server(
    addr: net::SocketAddr,
    repo_path: &Path,
    username: &str,
    password: &str,
) {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let server_handle = core.handle();

    let serve = loop {
        let repo_path = repo_path.to_owned();
        let h2 = core.handle();
        let username = username.to_owned();
        let password = password.to_owned();
        let make_service = move || {
            let cghttp = ServeTestGit {
                handle: h2.clone(),
                repo_path: repo_path.to_owned(),
                username: username.to_owned(),
                password: password.to_owned(),
            };
            Ok(cghttp)
        };
        if let Ok(serve) =
            Http::new().serve_addr_handle(&addr, &server_handle, make_service)
        {
            break serve;
        }
    };

    let h2 = server_handle.clone();
    server_handle.spawn(
        serve
            .for_each(move |conn| {
                h2.spawn(
                    conn.map(|_| ())
                        .map_err(|err| println!("serve error:: {:?}", err)),
                );
                Ok(())
            })
            .map_err(|_| ()),
    );

    core.run(futures::future::empty::<(), ()>()).unwrap();
}

fn run_server(args: Vec<String>) -> i32 {
    println!("RUN HTTP SERVER {:?}", &args);

    println!("args: {:?}", args);

    let args = clap::App::new("josh-test-server")
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
        )
        .get_matches_from(args);

    let port = args.value_of("port").unwrap_or("8000").to_owned();
    println!("Now listening on 0.0.0.0:{}", port);

    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    run_test_server(
        addr,
        &PathBuf::from(
            args.value_of("local").expect("missing local directory"),
        ),
        args.value_of("username").expect("missing username"),
        args.value_of("password").expect("missing password"),
    );

    return 0;
}

fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    exit(run_server(args));
}

fn do_cgi(
    req: Request,
    cmd: Command,
    handle: tokio_core::reactor::Handle,
) -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
    let mut cmd = cmd;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    cmd.stdin(Stdio::piped());
    cmd.env("SERVER_SOFTWARE", "hyper")
        .env("SERVER_NAME", "localhost") // TODO
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", "HTTP/1.1") // TODO
        .env("SERVER_PORT", "80") // TODO
        .env("REQUEST_METHOD", format!("{}", req.method()))
        .env("SCRIPT_NAME", "") // TODO
        .env("QUERY_STRING", req.query().unwrap_or(""))
        .env("REMOTE_ADDR", "") // TODO
        .env("AUTH_TYPE", "") // TODO
        .env("REMOTE_USER", "") // TODO
        .env(
            "CONTENT_TYPE",
            &format!(
                "{}",
                req.headers().get().unwrap_or(&ContentType::plaintext())
            ),
        )
        .env(
            "HTTP_CONTENT_ENCODING",
            &format!(
                "{}",
                req.headers().get().unwrap_or(&ContentEncoding(vec![]))
            ),
        )
        .env(
            "CONTENT_LENGTH",
            &format!("{}", req.headers().get().unwrap_or(&ContentLength(0))),
        );

    let mut child = cmd
        .spawn_async_with_handle(&handle.new_tokio_handle())
        .expect("can't spawn CGI command");

    let r = req.body().concat2().and_then(move |body| {
        tokio_io::io::write_all(child.stdin().take().unwrap(), body)
            .and_then(move |_| {
                child
                    .wait_with_output()
                    .map(build_response)
                    .map_err(|e| e.into())
            })
            .map_err(|e| e.into())
    });

    Box::new(r)
}

fn build_response(command_result: std::process::Output) -> Response {
    let mut stdout = io::BufReader::new(command_result.stdout.as_slice());
    let mut stderr = io::BufReader::new(command_result.stderr.as_slice());

    let mut response = Response::new();

    let mut headers = vec![];
    for line in stdout.by_ref().lines() {
        if line.as_ref().unwrap().is_empty() {
            break;
        }
        let l: Vec<&str> =
            line.as_ref().unwrap().as_str().splitn(2, ": ").collect();
        for x in &l {
            headers.push(x.to_string());
        }
        if l[0] == "Status" {
            response.set_status(hyper::StatusCode::Unregistered(
                u16::from_str(l[1].split(" ").next().unwrap()).unwrap(),
            ));
        } else {
            response
                .headers_mut()
                .set_raw(l[0].to_string(), l[1].to_string());
        }
    }

    let mut data = vec![];
    stdout
        .read_to_end(&mut data)
        .expect("can't read command output");

    let mut stderrdata = vec![];
    stderr
        .read_to_end(&mut stderrdata)
        .expect("can't read command output");

    response.set_body(hyper::Chunk::from(data));

    response
}
