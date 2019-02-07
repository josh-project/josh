/* #![deny(warnings)] */
extern crate clap;
extern crate crypto;
extern crate fern;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate regex;
extern crate tempdir;
extern crate tokio_core;

use rand::random;

use self::futures::future::Future;
use self::futures::Stream;
use self::futures_cpupool::CpuPool;
use self::hyper::header::{Authorization, Basic};
use self::hyper::server::{Http, Request, Response, Service};
use self::regex::Regex;
use super::cgi;
use super::scratch;
use super::virtual_repo;
use super::*;
use std::collections::HashMap;
use std::net;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use self::crypto::digest::Digest;
use self::crypto::sha1::Sha1;

lazy_static! {
    static ref VIEW_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<view>!.*)[.]git(?P<pathinfo>/.*)")
            .expect("can't compile regex");
    static ref FULL_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<pathinfo>/.*)").expect("can't compile regex");
}

struct HttpService {
    handle: tokio_core::reactor::Handle,
    pool: CpuPool,
    base_path: PathBuf,
    base_url: String,
    cache: Arc<Mutex<scratch::ViewCaches>>,
}

fn async_fetch(
    http: &HttpService,
    prefix: &str,
    view_string: &str,
    username: &str,
    password: &str,
    remote_url: String,
) -> Box<Future<Item = Result<PathBuf, git2::Error>, Error = hyper::Error>> {
    let br_path = http.base_path.join(prefix.trim_left_matches("/"));
    base_repo::create_local(&br_path);

    let username = username.to_owned();
    let password = password.to_owned();
    let cache = http.cache.clone();

    Box::new(http.pool.spawn(
        futures::future::ok(view_string.to_owned()).map(move |view_string| {
            match base_repo::fetch_refs_from_url(&br_path, &remote_url, &username, &password) {
                Ok(_) => Ok(make_view_repo(&view_string, &br_path, cache)),
                Err(e) => Err(e),
            }
        }),
    ))
}

fn respond_unauthorized() -> Response {
    let mut response: Response = Response::new().with_status(hyper::StatusCode::Unauthorized);
    response
        .headers_mut()
        .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\"");
    response
}

fn call_service(
    service: &HttpService,
    req: Request,
) -> Box<Future<Item = Response, Error = hyper::Error>> {
    let (prefix, view_string, pathinfo) = if let Some(caps) = VIEW_REGEX.captures(&req.uri().path())
    {
        (
            caps.name("prefix").unwrap().as_str().to_string(),
            caps.name("view").unwrap().as_str().to_string(),
            caps.name("pathinfo").unwrap().as_str().to_string(),
        )
    } else if let Some(caps) = FULL_REGEX.captures(&req.uri().path()) {
        (
            caps.name("prefix").unwrap().as_str().to_string(),
            "nop".to_string(),
            caps.name("pathinfo").unwrap().as_str().to_string(),
        )
    } else if req.uri().path() == "/version" {
        trace_scoped!("version");
        let response = Response::new()
            .with_body(format!("Version: {}\n", env!("VERSION")))
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    } else if req.uri().path() == "/panic" {
        panic!();
    } else {
        let response = Response::new().with_status(hyper::StatusCode::NotFound);
        return Box::new(futures::future::ok(response));
    };

    let (username, password) = match req.headers().get() {
        Some(&Authorization(Basic {
            ref username,
            ref password,
        })) => (
            username.to_owned(),
            password.to_owned().unwrap_or("".to_owned()).to_owned(),
        ),
        _ => {
            return Box::new(futures::future::ok(respond_unauthorized()));
        }
    };

    let passwd = password.clone();
    let usernm = username.clone();
    let viewstr = view_string.clone();

    let remote_url = {
        let mut remote_url = service.base_url.clone();
        remote_url.push_str(&prefix);
        remote_url
    };

    let br_url = remote_url.clone();

    let ns = {
        let mut hasher = Sha1::new();
        hasher.input_str(&viewstr);
        hasher.result_str().to_string()
    };

    let call_git_http_backend = |request: Request,
                                 path: PathBuf,
                                 pathinfo: &str,
                                 handle: &tokio_core::reactor::Handle|
     -> Box<Future<Item = Response, Error = hyper::Error>> {
        println!("CALLING git-http backend {:?} {:?}", path, pathinfo);
        let mut cmd = Command::new("git");
        cmd.arg("http-backend");
        cmd.current_dir(&path);
        cmd.env("GIT_PROJECT_ROOT", path.to_str().unwrap());
        cmd.env("GIT_DIR", path.to_str().unwrap());
        cmd.env("GIT_HTTP_EXPORT_ALL", "");
        cmd.env("PATH_INFO", pathinfo);
        cmd.env("JOSH_PASSWORD", passwd);
        cmd.env("JOSH_USERNAME", usernm);
        cmd.env("GIT_NAMESPACE", ns);
        cmd.env("JOSH_VIEWSTR", viewstr);
        cmd.env("JOSH_REMOTE", remote_url);

        cgi::do_cgi(request, cmd, handle.clone())
    };

    println!("PREFIX: {}", &prefix);
    println!("VIEW: {}", &view_string);
    println!("PATH_INFO: {:?}", &pathinfo);

    let handle = service.handle.clone();

    Box::new({
        async_fetch(
            &service,
            &prefix,
            &view_string,
            &username,
            &password,
            br_url,
        )
        .and_then(move |view_repo| match view_repo {
            Err(_e) => {
                println!("wrong credentials");
                trace_end!("request", "msg": "wrong credentials");
                Box::new(futures::future::ok(respond_unauthorized()))
            }

            Ok(path) => call_git_http_backend(req, path, &pathinfo, &handle),
        })
    })
}

impl Service for HttpService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let rid: usize = random();
        let rname = format!("request {}", rid);

        let username = match req.headers().get() {
            Some(&Authorization(Basic {
                ref username,
                ref password,
            })) => username.to_owned(),
            None => "".to_owned(),
        };
        let mut headers = req.headers().clone();
        headers.set(Authorization(Basic {
            username: username,
            password: None,
        }));

        trace_begin!(&rname, "path": req.path(), "headers": format!("{:?}", &headers));
        Box::new(call_service(&self, req).map(move |x| {
            trace_end!(&rname, "response": format!("{:?}", x));
            x
        }))
    }
}

pub fn run_proxy(args: Vec<String>) -> i32 {
    println!("RUN PROXY {:?}", &args);
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
        .chain(std::io::stdout())
        .chain(fern::log_file(logfilename).unwrap())
        .apply()
        .unwrap();

    debug!("args: {:?}", args);

    if args[0].ends_with("/update") {
        debug!("================= HOOK {:?}", args);
        return virtual_repo::update_hook(&args[1], &args[2], &args[3]);
    }

    let args = clap::App::new("josh-proxy")
        .arg(
            clap::Arg::with_name("remote")
                .long("remote")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("local")
                .long("local")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("trace")
                .long("trace")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .get_matches_from(args);

    let port = args.value_of("port").unwrap_or("8000").to_owned();
    println!("Now listening on localhost:{}", port);

    let pool = CpuPool::new(1);

    if let Some(tf) = args.value_of("trace") {
        open_trace_file!(tf).expect("can't open tracefile");

        let h = panic::take_hook();
        panic::set_hook(Box::new(move |x| {
            close_trace_file!();
            h(x);
        }));
    }

    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    run_http_server(
        addr,
        &pool,
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
    );

    return 0;
}

fn run_http_server(addr: net::SocketAddr, pool: &CpuPool, local: &Path, remote: &str) {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let h2 = core.handle();
    let cache = Arc::new(Mutex::new(HashMap::new()));
    let server_handle = core.handle();
    let pool = pool.clone();
    let remote = remote.to_owned();
    let local = local.to_owned();
    let serve = Http::new()
        .serve_addr_handle(&addr, &server_handle, move || {
            let cghttp = HttpService {
                handle: h2.clone(),
                pool: pool.clone(),
                base_path: local.clone(),
                base_url: remote.clone(),
                cache: cache.clone(),
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
}

fn make_view_repo(
    view_string: &str,
    br_path: &Path,
    cache: Arc<Mutex<scratch::ViewCaches>>,
) -> PathBuf {
    trace_scoped!(
        "make_view_repo",
        "view_string": view_string,
        "br_path": br_path
    );

    let scratch = scratch::new(&br_path);

    for branch in scratch.branches(None).unwrap() {
        scratch::apply_view_to_branch(
            &scratch,
            &branch.unwrap().0.name().unwrap().unwrap(),
            &view_string,
            &mut cache.lock().unwrap(),
        );
    }

    virtual_repo::setup_tmp_repo(&br_path);
    br_path.to_owned()
}
