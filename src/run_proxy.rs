/* #![deny(warnings)] */
extern crate clap;
extern crate fern;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate regex;
extern crate tempdir;
extern crate tokio_core;


use self::futures::Stream;
use self::futures::future::Future;
use self::futures_cpupool::CpuPool;
use self::hyper::header::{Authorization, Basic};
use self::hyper::server::{Http, Request, Response, Service};
use self::regex::Regex;
use super::*;
use super::cgi;
use super::scratch;
use super::virtual_repo;
use std::collections::HashMap;
use std::net;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

lazy_static! {
    static ref VIEW_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)/(?P<view>.*)[.]git(?P<pathinfo>/.*)")
            .expect("can't compile regex");
    static ref FULL_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<pathinfo>/.*)")
            .expect("can't compile regex");
}

struct GribHttp
{
    handle: tokio_core::reactor::Handle,
    pool: CpuPool,
    base_path: PathBuf,
    base_url: String,
    cache: Arc<Mutex<scratch::ViewCaches>>,
}

fn async_fetch(
    http: &GribHttp,
    prefix: &str,
    view_string: &str,
    username: &str,
    password: &str,
    remote_url: String,
) -> Box<Future<Item = Result<PathBuf, git2::Error>, Error = hyper::Error>>
{
    let br_path = http.base_path.join(prefix.trim_left_matches("/"));
    base_repo::create_local(&br_path);

    let username = username.to_owned();
    let password = password.to_owned();
    let cache = http.cache.clone();

    Box::new(http.pool.spawn(
        futures::future::ok(view_string.to_owned()).map(move |view_string| {
            match base_repo::fetch_refs_from_url(&br_path, &remote_url, &username, &password) {
                Ok(_) => Ok(
                    make_view_repo(&view_string, &br_path, cache),
                ),
                Err(e) => Err(e),
            }
        }),
    ))
}

fn respond_unauthorized() -> Response
{
    let mut response: Response = Response::new().with_status(hyper::StatusCode::Unauthorized);
    response
        .headers_mut()
        .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\"");
    response
}


impl Service for GribHttp
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;

    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;


    fn call(&self, req: Request) -> Self::Future
    {
        let (username, password) = match req.headers().get() {
            Some(&Authorization(Basic {
                ref username,
                ref password,
            })) => (username.to_owned(), password.to_owned().unwrap_or("".to_owned()).to_owned()),
            _ => {
                println!("no credentials in request");
                return Box::new(futures::future::ok(respond_unauthorized()));
            }
        };


        let (prefix, view_string, pathinfo) =
            if let Some(caps) = VIEW_REGEX.captures(&req.uri().path()) {
                (
                    caps.name("prefix").unwrap().as_str().to_string(),
                    caps.name("view").unwrap().as_str().to_string(),
                    caps.name("pathinfo").unwrap().as_str().to_string(),
                )
            } else if let Some(caps) = FULL_REGEX.captures(&req.uri().path()) {
                (
                    caps.name("prefix").unwrap().as_str().to_string(),
                    ".".to_string(),
                    caps.name("pathinfo").unwrap().as_str().to_string(),
                )
            } else {
                let response = Response::new().with_status(hyper::StatusCode::NotFound);
                return Box::new(futures::future::ok(response));
            };

        let passwd = password.clone();
        let usernm = username.clone();
        let viewstr = view_string.clone();
        let br_path = self.base_path.join(prefix.trim_left_matches("/"));

        let remote_url = {
            let mut remote_url = self.base_url.clone();
            remote_url.push_str(&prefix);
            remote_url
        };

        let br_url = remote_url.clone();

        let call_git_http_backend =
            |request: Request,
             path: PathBuf,
             pathinfo: &str,
             handle: &tokio_core::reactor::Handle| {
                println!("CALLING git-http backend {:?} {:?}", path, pathinfo);
                let mut cmd = Command::new("git");
                cmd.arg("http-backend");
                cmd.current_dir(&path);
                cmd.env("GIT_PROJECT_ROOT", path.to_str().unwrap());
                cmd.env("GIT_DIR", path.to_str().unwrap());
                cmd.env("GIT_HTTP_EXPORT_ALL", "");
                cmd.env("PATH_INFO", pathinfo);
                cmd.env("GRIB_PASSWORD", passwd);
                cmd.env("GRIB_USERNAME", usernm);
                cmd.env("GRIB_VIEW", viewstr);
                cmd.env("GRIB_BR_PATH", br_path);
                cmd.env("GRIB_REMOTE", remote_url);

                cgi::do_cgi(request, cmd, handle.clone())
            };

        println!("PREFIX: {}", &prefix);
        println!("VIEW: {}", &view_string);
        println!("PATH_INFO: {:?}", &pathinfo);



        let handle = self.handle.clone();


        Box::new({
            async_fetch(&self, &prefix, &view_string, &username, &password, br_url).and_then(
                move |view_repo| match view_repo {
                    Err(e) =>
                    {
                        println!("wrong credentials");
                        Box::new(futures::future::ok(respond_unauthorized()))
                    },

                    Ok(path) => call_git_http_backend(req, path, &pathinfo, &handle),
                },
            )
        })
    }
}

pub fn run_proxy(args: Vec<String>) -> i32
{
    println!("RUN PROXY {:?}", &args);
    let logfilename = Path::new("/tmp/centralgit.log");
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!("{}[{}] {}", record.target(), record.level(), message))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .chain(fern::log_file(logfilename).unwrap())
        .apply()
        .unwrap();


    debug!("args: {:?}", args);

    if args[0].ends_with("/update") {
        debug!("================= HOOK {:?}", args);
        return virtual_repo::update_hook(&args[1], &args[2], &args[3]);
    }

    let args = clap::App::new("grib")
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
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .get_matches_from(args);


    let port = args.value_of("port").unwrap_or("8000").to_owned();
    println!("Now listening on localhost:{}", port);

    let pool = CpuPool::new(1);

    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    run_http_server(
        addr,
        &pool,
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
    );


    return 0;
}


fn run_http_server(addr: net::SocketAddr, pool: &CpuPool, local: &Path, remote: &str)
{
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let h2 = core.handle();
    let cache = Arc::new(Mutex::new(HashMap::new()));
    let server_handle = core.handle();
    let pool = pool.clone();
    let remote = remote.to_owned();
    let local = local.to_owned();
    let serve = Http::new()
        .serve_addr_handle(&addr, &server_handle, move || {
            let cghttp = GribHttp {
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
) -> PathBuf
{
    let scratch = scratch::new(&br_path);

    for branch in scratch.branches(None).unwrap() {
        scratch::apply_view_to_branch(
            &scratch,
            &branch.unwrap().0.name().unwrap().unwrap(),
            &view_string,
            &mut cache.lock().unwrap(),
        );
    }

    virtual_repo::setup_tmp_repo(&br_path, &view_string)
}
