/* #![deny(warnings)] */
extern crate clap;
extern crate crypto;
extern crate fern;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate regex;
extern crate serde_json;
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
use std::env::current_exe;
use std::fs::remove_dir_all;
use std::net;
use std::os::unix::fs::symlink;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

lazy_static! {
    static ref VIEW_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<view>.*)[.]git(?P<pathinfo>/.*)")
            .expect("can't compile regex");
    static ref FULL_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<pathinfo>/.*)").expect("can't compile regex");
}

struct HttpService {
    handle: tokio_core::reactor::Handle,
    pool: CpuPool,
    port: String,
    base_path: PathBuf,
    base_url: String,
    forward_maps: Arc<Mutex<scratch::ViewMaps>>,
    backward_maps: Arc<Mutex<scratch::ViewMaps>>,
}

fn async_fetch(
    http: &HttpService,
    prefix: &str,
    view_string: &str,
    username: &str,
    password: &str,
    namespace: &str,
    remote_url: String,
) -> Box<Future<Item = Result<PathBuf, git2::Error>, Error = hyper::Error>> {
    let br_path = http.base_path.join(prefix.trim_left_matches("/"));
    base_repo::create_local(&br_path);

    let username = username.to_owned();
    let password = password.to_owned();
    let forward_maps = http.forward_maps.clone();
    let backward_maps = http.backward_maps.clone();
    let namespace = namespace.to_owned();

    Box::new(http.pool.spawn(
        futures::future::ok(view_string.to_owned()).map(move |view_string| {
            match base_repo::fetch_refs_from_url(&br_path, &remote_url, &username, &password) {
                Ok(_) => Ok(make_view_repo(
                    &view_string,
                    &namespace,
                    &br_path,
                    forward_maps,
                    backward_maps,
                )),
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

fn parse_url(path: &str) -> Option<(String, String, String)> {
    if let Some(caps) = VIEW_REGEX.captures(&path) {
        return Some((
            caps.name("prefix").unwrap().as_str().to_string(),
            caps.name("view").unwrap().as_str().to_string(),
            caps.name("pathinfo").unwrap().as_str().to_string(),
        ));
    }

    if let Some(caps) = FULL_REGEX.captures(&path) {
        return Some((
            caps.name("prefix").unwrap().as_str().to_string(),
            "!nop".to_string(),
            caps.name("pathinfo").unwrap().as_str().to_string(),
        ));
    }
    return None;
}

fn call_service(
    service: &HttpService,
    req: Request,
    namespace: &str,
) -> Box<Future<Item = Response, Error = hyper::Error>> {
    let backward_maps = service.backward_maps.clone();
    if req.uri().path() == "/version" {
        let response = Response::new()
            .with_body(format!("Version: {}\n", env!("VERSION")))
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if req.uri().path() == "/panic" {
        panic!();
    }
    if req.uri().path() == "/repo_update" {
        let pool = service.pool.clone();
        return Box::new(
            req.body()
                .concat2()
                .map(move |body| {
                    let mut buffer: Vec<u8> = Vec::new();
                    for i in body {
                        buffer.push(i);
                    }

                    String::from_utf8(buffer).unwrap_or("".to_string())
                })
                .and_then(move |buffer| {
                    return pool.spawn(futures::future::ok(buffer).map(move |buffer| {
                        let repo_update: virtual_repo::RepoUpdate = serde_json::from_str(&buffer)
                            .unwrap_or(virtual_repo::RepoUpdate::new());
                        virtual_repo::process_repo_update(repo_update, backward_maps)
                    }));
                })
                .and_then(move |result| {
                    if let Ok(stderr) = result {
                        let response = Response::new()
                            .with_body(stderr)
                            .with_status(hyper::StatusCode::Ok);
                        return Box::new(futures::future::ok(response));
                    }
                    let response = Response::new().with_status(hyper::StatusCode::Forbidden);
                    return Box::new(futures::future::ok(response));
                }),
        );
    }

    let (prefix, view_string, pathinfo) = some_or!(parse_url(&req.uri().path()), {
        let response = Response::new().with_status(hyper::StatusCode::NotFound);
        return Box::new(futures::future::ok(response));
    });

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
    let ns = namespace.to_owned();

    let port = service.port.clone();

    let remote_url = {
        let mut remote_url = service.base_url.clone();
        remote_url.push_str(&prefix);
        remote_url
    };

    let br_url = remote_url.clone();

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
        cmd.env("JOSH_PORT", port);
        cmd.env("GIT_NAMESPACE", ns);
        cmd.env("JOSH_VIEWSTR", viewstr);
        cmd.env("JOSH_REMOTE", remote_url);

        cgi::do_cgi(request, cmd, handle.clone())
    };

    println!("PREFIX: {}", &prefix);
    println!("VIEW: {}", &view_string);
    println!("PATH_INFO: {:?}", &pathinfo);

    let handle = service.handle.clone();
    let ns_path = service.base_path.join(prefix.trim_left_matches("/"));
    let ns_path = ns_path.join("refs/namespaces");
    let ns_path = ns_path.join(&namespace);

    Box::new({
        async_fetch(
            &service,
            &prefix,
            &view_string,
            &username,
            &password,
            &namespace,
            br_url,
        )
        .and_then(
            move |view_repo| -> Box<Future<Item = Response, Error = hyper::Error>> {
                let path = ok_or!(view_repo, {
                    println!("wrong credentials");
                    return Box::new(futures::future::ok(respond_unauthorized()));
                });

                call_git_http_backend(req, path, &pathinfo, &handle)
            },
        )
        .map(move |x| {
            remove_dir_all(ns_path);
            x
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
        let rname = format!("request_{}", rid);

        let username = match req.headers().get() {
            Some(&Authorization(Basic {
                ref username,
                password: _,
            })) => username.to_owned(),
            None => "".to_owned(),
        };
        let mut headers = req.headers().clone();
        headers.set(Authorization(Basic {
            username: username,
            password: None,
        }));

        trace_begin!(&rname, "path": req.path(), "headers": format!("{:?}", &headers));
        Box::new(call_service(&self, req, &rname).map(move |x| {
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
        port,
        &pool,
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
    );

    return 0;
}

fn run_http_server(
    addr: net::SocketAddr,
    port: String,
    pool: &CpuPool,
    local: &Path,
    remote: &str,
) {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let h2 = core.handle();
    let forward_maps = Arc::new(Mutex::new(HashMap::new()));
    let backward_maps = Arc::new(Mutex::new(HashMap::new()));
    let server_handle = core.handle();
    let pool = pool.clone();
    let port = port.clone();
    let remote = remote.to_owned();
    let local = local.to_owned();
    let serve = Http::new()
        .serve_addr_handle(&addr, &server_handle, move || {
            let cghttp = HttpService {
                handle: h2.clone(),
                pool: pool.clone(),
                port: port.clone(),
                base_path: local.clone(),
                base_url: remote.clone(),
                forward_maps: forward_maps.clone(),
                backward_maps: backward_maps.clone(),
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
    namespace: &str,
    br_path: &Path,
    forward_maps: Arc<Mutex<scratch::ViewMaps>>,
    backward_maps: Arc<Mutex<scratch::ViewMaps>>,
) -> PathBuf {
    trace_scoped!(
        "make_view_repo",
        "view_string": view_string,
        "br_path": br_path
    );

    let scratch = scratch::new(&br_path);

    let mut forward_maps = forward_maps.lock().unwrap();
    let mut backward_maps = backward_maps.lock().unwrap();

    let viewobj = build_view(&view_string);

    let mut bm = backward_maps
        .entry(viewobj.viewstr())
        .or_insert_with(ViewMap::new);

    for branch in scratch.branches(None).unwrap() {
        scratch::apply_view_to_branch(
            &scratch,
            &branch.unwrap().0.name().unwrap().unwrap(),
            &*viewobj,
            &mut forward_maps,
            &mut bm,
            &namespace,
        );
    }

    let mut forward_map = forward_maps
        .entry(viewobj.viewstr())
        .or_insert_with(ViewMap::new);

    for tag in scratch.tag_names(None).expect("scratch.tag_names").iter() {
        let tag = some_or!(tag, {
            continue;
        });
        let r = ok_or!(scratch.find_reference(&format!("refs/tags/{}", tag)), {
            continue;
        });
        let target = some_or!(r.target(), {
            continue;
        });

        if let Some(n) = forward_map.get(&target) {
            if *n == git2::Oid::zero() {
                continue;
            }
            ok_or!(
                scratch.reference(
                    &format!("refs/namespaces/{}/refs/tags/{}", &namespace, &tag),
                    *n,
                    true,
                    "crate tag",
                ),
                {
                    continue;
                }
            );
        }
    }

    setup_tmp_repo(&br_path);
    br_path.to_owned()
}

fn setup_tmp_repo(scratch_dir: &Path) {
    let shell = Shell {
        cwd: scratch_dir.to_path_buf(),
    };

    if !scratch_dir.join("josh_hook_installed").exists() {
        shell.command("touch josh_hook_installed");
        shell.command("git config http.receivepack true");
        let ce = current_exe().expect("can't find path to exe");
        shell.command("rm -Rf hooks");
        shell.command("mkdir hooks");
        symlink(ce, scratch_dir.join("hooks").join("update")).expect("can't symlink update hook");
    }
}
