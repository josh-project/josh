/* #![deny(warnings)] */
extern crate bobble;
extern crate clap;
extern crate fern;
extern crate futures;
extern crate git2;
extern crate hyper;
extern crate regex;
extern crate tempdir;
extern crate tokio_core;
extern crate futures_cpupool;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

use bobble::*;
use bobble::virtual_repo;
use futures::Stream;
use futures::future::Future;
use hyper::server::{Http, Request, Response, Service};
use regex::Regex;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::exit;
use futures_cpupool::CpuPool;

lazy_static! {
    static ref PREFIX_RE: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)/.*").expect("can't compile regex");
    static ref VIEW_RE: Regex =
        Regex::new(r"/(?P<view>.*)[.]git/.*").expect("can't compile regex");
}

fn main()
{
    exit(main_ret());
}

struct BobbleHttp
{
    handle: tokio_core::reactor::Handle,
    pool: CpuPool,
    base_repo: BaseRepo,
}

impl BobbleHttp
{
    fn async_fetch(&self, path: &str)
    -> Box<Future<Item = PathBuf, Error = hyper::Error>> {
        let base_repo = self.base_repo.clone();

        Box::new(
            self.pool.spawn(futures::future::ok(path.to_owned()).map(move |path|{
                base_repo.fetch_origin_master();

                make_view_repo(
                    &path,
                    &base_repo.path,
                    &base_repo.user,
                    &base_repo.password,
                    &base_repo.url,
                )
            }))
        )
    }

    /* fn delegate_auth(&self, user: &str, password: &str) */
    /* -> Box<Future<Item = Response, Error = hyper::Error>> */
    /* { */
    /*     Box::new(hyper::client::Client::new(&self.handle).get("http://gerrit/".parse().unwrap())) */
    /* } */
}


impl Service for BobbleHttp
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;


    fn call(&self, req: Request) -> Self::Future
    {

        let prefix = if let Some(caps) = PREFIX_RE.captures(&req.uri().path()) {
            caps.name("prefix")
                .expect("can't find name prefix")
                .as_str()
                .to_string()
        } else {
            String::new()
        };

        let path_without_prefix = if prefix != "" {
            req.uri().path().replacen(&prefix, "", 1)
        } else {
            req.uri().path().to_owned()
        };

        let handle = self.handle.clone();

        Box::new({
            self.async_fetch(&req.uri().path()).and_then(move |view_repo|{

                let mut cmd = Command::new("git");
                cmd.arg("http-backend");
                cmd.current_dir(&view_repo);
                cmd.env("GIT_PROJECT_ROOT", view_repo.to_str().unwrap());
                cmd.env("GIT_DIR", view_repo.to_str().unwrap());
                cmd.env("GIT_HTTP_EXPORT_ALL", "");
                cmd.env("PATH_INFO", path_without_prefix);

                cgi::do_cgi(req, cmd, handle.clone())
            })
        })

    }
}

fn main_ret() -> i32
{
    let pool = CpuPool::new(1);

    let mut args = vec![];
    for arg in env::args() {
        args.push(arg);
    }
    debug!("args: {:?}", args);

    let logfilename = Path::new("/tmp/centralgit.log");
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .chain(fern::log_file(logfilename).unwrap())
        .apply()
        .unwrap();

    if args[0].ends_with("/update") {
        debug!("================= HOOK {:?}", args);
        return virtual_repo::update_hook(&args[1], &args[2], &args[3]);
    }

    let args = clap::App::new("centralgit-http")
        .arg(clap::Arg::with_name("remote").long("remote").takes_value(true))
        .arg(clap::Arg::with_name("local").long("local").takes_value(true))
        .arg(clap::Arg::with_name("user").long("user").takes_value(true))
        .arg(clap::Arg::with_name("password").long("password").takes_value(true))
        .get_matches();

    let user = args.value_of("user")
        .expect("missing user name")
        .to_string();
    let password = args.value_of("password").expect("missing http password").to_string();


    println!("Now listening on localhost:8000");

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let addr = "127.0.0.1:8000".parse().unwrap();
    let server_handle = core.handle();
    let h2 = core.handle();

    let base_repo = BaseRepo::create(
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
        &user,
        &password,
    );
    base_repo.git_clone();

    let serve = Http::new()
        .serve_addr_handle(&addr, &server_handle, move || {
            let cghttp = BobbleHttp {
                handle: h2.clone(),
                pool: pool.clone(),
                base_repo: BaseRepo::create(
                    &PathBuf::from(args.value_of("local").expect("missing local directory")),
                    &args.value_of("remote").expect("missing remote repo url"),
                    &user,
                    &password,
                ),
            };
            Ok(cghttp)
        })
        .unwrap();

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

    return 0;
}

fn make_view_repo(url: &str, base: &Path, user: &str, password: &str, remote_url: &str) -> PathBuf
{
    let view_string = if let Some(caps) = VIEW_RE.captures(&url) {
        caps.name("view").unwrap().as_str().to_owned()
    } else {
        ".".to_owned()
    };

    println!("VIEW {}", &view_string);

    let scratch = Scratch::new(&base);
    for branch in scratch.repo.branches(None).unwrap() {
        scratch.apply_view_to_branch(&branch.unwrap().0.name().unwrap().unwrap(), &view_string);
    }

    virtual_repo::setup_tmp_repo(&base, &view_string, &user, &password, &remote_url)
}
