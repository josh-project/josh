#![deny(warnings)]
#![allow(clippy::needless_return)]
extern crate clap;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate josh;
extern crate lazy_static;
extern crate regex;
extern crate tokio_core;

#[macro_use]
extern crate log;

use self::futures::future::Future;
use self::futures::Stream;
use self::hyper::header::{Authorization, Basic};
use self::hyper::server::Http;
use self::hyper::server::{Request, Response, Service};
use josh::*;
use std::env;
use std::net;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;
use std::process::Command;

pub struct ServeTestGit {
    handle: tokio_core::reactor::Handle,
    repo_path: PathBuf,
    username: String,
    password: String,
}

impl Service for ServeTestGit {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        println!("call");
        let (username, password) = match req.headers().get() {
            Some(&Authorization(Basic {
                ref username,
                ref password,
            })) => (
                username.to_owned(),
                password.to_owned().unwrap_or_else(|| "".to_owned()),
            ),
            _ => {
                println!("ServeTestGit: no credentials in request");
                let mut response = Response::new()
                    .with_status(hyper::StatusCode::Unauthorized);
                response.headers_mut().set_raw(
                    "WWW-Authenticate",
                    "Basic realm=\"User Visible Realm\"",
                );
                return Box::new(futures::future::ok(response));
            }
        };

        if username != "admin"
            && (username != self.username || password != self.password)
        {
            println!("ServeTestGit: wrong user/pass");
            println!("user: {:?} - {:?}", username, self.username);
            println!("pass: {:?} - {:?}", password, self.password);
            let mut response =
                Response::new().with_status(hyper::StatusCode::Unauthorized);
            response.headers_mut().set_raw(
                "WWW-Authenticate",
                "Basic realm=\"User Visible Realm\"",
            );
            return Box::new(futures::future::ok(response));
        }

        println!("CREDENTIALS OK {:?} {:?}", &username, &password);

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

        cgi::do_cgi(req, cmd, handle.clone())
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
    debug!("RUN HTTP SERVER {:?}", &args);

    debug!("args: {:?}", args);

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
