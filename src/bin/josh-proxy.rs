#![deny(warnings)]

#[macro_use]
extern crate josh;

#[macro_use]
extern crate rs_tracing;

extern crate clap;
extern crate fern;
extern crate futures;
extern crate futures_cpupool;
extern crate git2;
extern crate hyper;
extern crate rand;
extern crate regex;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate serde_json;

extern crate crypto;
extern crate tempdir;
extern crate tokio_core;

use futures::future::Future;
use futures::Stream;
use futures_cpupool::CpuPool;
use hyper::header::{Authorization, Basic};
use hyper::server::{Http, Request, Response, Service};
use josh::base_repo;
use josh::cgi;
use josh::scratch;
use josh::shell;
use josh::view_maps;
use josh::virtual_repo;
use rand::random;
use regex::Regex;
use std::env;
use std::process::exit;

use crypto::digest::Digest;
use std::collections::HashMap;
use std::env::current_exe;
use std::fs::remove_dir_all;
use std::net;
use std::os::unix::fs::symlink;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, RwLock};

lazy_static! {
    static ref VIEW_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<view>.*)[.]git(?P<pathinfo>/.*)")
            .expect("can't compile regex");
    static ref INFO_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<view>.*)[.]json@(?P<ref>.*)")
            .expect("can't compile regex");
    static ref FULL_REGEX: Regex =
        Regex::new(r"(?P<prefix>/.*[.]git)(?P<pathinfo>/.*)").expect("can't compile regex");
}

type CredentialCache = HashMap<String, std::time::Instant>;

struct HttpService {
    handle: tokio_core::reactor::Handle,
    fetch_push_pool: CpuPool,
    compute_pool: CpuPool,
    port: String,
    base_path: PathBuf,
    base_url: String,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
}

fn hash_credentials(url: &str, username: &str, password: &str) -> String {
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

fn fetch_upstream(
    http: &HttpService,
    prefix: &str,
    username: &str,
    password: &str,
    remote_url: String,
) -> Box<futures_cpupool::CpuFuture<std::result::Result<(), git2::Error>, hyper::Error>> {
    let credentials_hashed = hash_credentials(&remote_url, &username, &password);
    let username = username.to_owned();
    let password = password.to_owned();
    let prefix = prefix.to_owned();
    let br_path = http.base_path.clone();
    let credential_cache = http.credential_cache.clone();

    let do_fetch = Box::new(
        http.fetch_push_pool
            .spawn(futures::future::ok(()).map(move |_| {
                base_repo::fetch_refs_from_url(
                    &br_path,
                    &prefix,
                    &remote_url,
                    &"refs/heads/*",
                    &username,
                    &password,
                )
                .and_then(|_| {
                    base_repo::fetch_refs_from_url(
                        &br_path,
                        &prefix,
                        &remote_url,
                        &"refs/tags/*",
                        &username,
                        &password,
                    )
                })
                .and_then(|_| {
                    base_repo::fetch_refs_from_url(
                        &br_path,
                        &prefix,
                        &remote_url,
                        &"HEAD",
                        &username,
                        &password,
                    )
                })
                .and_then(|_| {
                    let credentials_hashed = hash_credentials(&remote_url, &username, &password);
                    if let Ok(mut cc) = credential_cache.write() {
                        cc.insert(credentials_hashed, std::time::Instant::now());
                    }
                    Ok(())
                })
            })),
    );

    let last = http
        .credential_cache
        .read()
        .ok()
        .map(|cc| cc.get(&credentials_hashed).copied());

    if let Some(Some(c)) = last {
        if std::time::Instant::now().duration_since(c) < std::time::Duration::from_secs(30) {
            do_fetch.forget();
            return Box::new(http.compute_pool.spawn(futures::future::ok(Ok(()))));
        }
    }

    return do_fetch;
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
    let br_path = http.base_path.clone();
    base_repo::create_local(&br_path);

    let fetch_future = fetch_upstream(http, prefix, username, password, remote_url);

    let viewstr = view_string.to_owned();
    let forward_maps = http.forward_maps.clone();
    let backward_maps = http.backward_maps.clone();
    let namespace = namespace.to_owned();
    let br_path = http.base_path.clone();
    let prefix = prefix.to_owned();
    Box::new(http.compute_pool.spawn(fetch_future.map(move |r| {
        r.map(move |_| {
            make_view_repo(
                &viewstr,
                &prefix,
                &namespace,
                &br_path,
                forward_maps,
                backward_maps,
            )
        })
    })))
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

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if path == "/version" {
        let response = Response::new()
            .with_body(format!("Version: {}\n", env!("VERSION")))
            .with_status(hyper::StatusCode::Ok);
        return Box::new(futures::future::ok(response));
    }
    if path == "/panic" {
        panic!();
    }
    if path == "/repo_update" {
        let pool = service.fetch_push_pool.clone();
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
                        let backward_maps = backward_maps.read().unwrap();
                        virtual_repo::process_repo_update(repo_update, &backward_maps)
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

    let compute_pool = service.compute_pool.clone();

    if let Some(caps) = INFO_REGEX.captures(&path) {
        let (prefix, view, rev) = (
            caps.name("prefix").unwrap().as_str().to_string(),
            caps.name("view").unwrap().as_str().to_string(),
            caps.name("ref").unwrap().as_str().to_string(),
        );

        let forward_maps = service.forward_maps.clone();
        let backward_maps = service.forward_maps.clone();
        let br_path = service.base_path.clone();

        let f = compute_pool.spawn(futures::future::ok(true).map(move |_| {
            let info = get_info(
                &view,
                &prefix,
                &rev,
                &br_path,
                forward_maps.clone(),
                backward_maps.clone(),
            );
            info
        }));

        return Box::new(f.and_then(move |info| {
            let response = Response::new()
                .with_body(format!("{}\n", info))
                .with_status(hyper::StatusCode::Ok);
            return Box::new(futures::future::ok(response));
        }));
    }

    let (prefix, view_string, pathinfo) = some_or!(parse_url(&path), {
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
    let ns_path = service.base_path.clone();
    let ns_path = ns_path.join("refs/namespaces");
    let ns_path = ns_path.join(&namespace);
    assert!(namespace.contains("request_"));

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

                install_josh_hook(&path);
                call_git_http_backend(req, path, &pathinfo, &handle)
            },
        )
        .map(move |x| {
            if true {
                remove_dir_all(ns_path)
                    .unwrap_or_else(|e| println!("remove_dir_all failed: {:?}", e));
            }
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

fn run_proxy(args: Vec<String>) -> i32 {
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
        &PathBuf::from(args.value_of("local").expect("missing local directory")),
        &args.value_of("remote").expect("missing remote repo url"),
    );

    return 0;
}

fn run_http_server(addr: net::SocketAddr, port: String, local: &Path, remote: &str) {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let h2 = core.handle();
    let forward_maps = Arc::new(RwLock::new(view_maps::ViewMaps::new()));
    let backward_maps = Arc::new(RwLock::new(view_maps::ViewMaps::new()));
    let credential_cache = Arc::new(RwLock::new(CredentialCache::new()));
    let server_handle = core.handle();
    let fetch_push_pool = CpuPool::new(1);
    let compute_pool = CpuPool::new(4);
    let port = port.clone();
    let remote = remote.to_owned();
    let local = local.to_owned();
    let serve = Http::new()
        .serve_addr_handle(&addr, &server_handle, move || {
            let cghttp = HttpService {
                handle: h2.clone(),
                fetch_push_pool: fetch_push_pool.clone(),
                compute_pool: compute_pool.clone(),
                port: port.clone(),
                base_path: local.clone(),
                base_url: remote.clone(),
                forward_maps: forward_maps.clone(),
                backward_maps: backward_maps.clone(),
                credential_cache: credential_cache.clone(),
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

fn to_ns(path: &str) -> String {
    return path.trim_matches('/').replace("/", "/refs/namespaces/");
}

fn make_view_repo(
    view_string: &str,
    prefix: &str,
    namespace: &str,
    br_path: &Path,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> PathBuf {
    trace_scoped!(
        "make_view_repo",
        "view_string": view_string,
        "br_path": br_path
    );

    let scratch = scratch::new(&br_path);

    let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone());
    let mut fm = view_maps::ViewMaps::new_downstream(forward_maps.clone());

    let viewobj = josh::build_view(&scratch, &view_string);

    let mut refs = vec![];

    let refname = format!("refs/namespaces/{}/HEAD", &to_ns(prefix));
    let to_ref = refname.replacen(&to_ns(prefix), &namespace, 1);
    refs.push((refname.to_owned(), to_ref.clone()));

    let glob = format!("refs/namespaces/{}/*", &to_ns(prefix));
    for refname in scratch.references_glob(&glob).unwrap().names() {
        let refname = refname.unwrap();
        let to_ref = refname.replacen(&to_ns(prefix), &namespace, 1);

        if to_ref.contains("/refs/cache-automerge/") {
            continue;
        }
        if to_ref.contains("/refs/notes/") {
            continue;
        }

        refs.push((refname.to_owned(), to_ref.clone()));
        if to_ref.contains("/refs/heads/") {
            refs.push((
                refname.to_owned(),
                to_ref.replace("/refs/heads/", "/refs/for/"),
            ));
            refs.push((
                refname.to_owned(),
                to_ref.replace("/refs/heads/", "/refs/drafts/"),
            ));
        }
    }

    scratch::apply_view_to_refs(&scratch, &*viewobj, &refs, &mut fm, &mut bm);

    trace_begin!(
        "merge_maps",
        "before_fm": forward_maps.read().unwrap().stats(),
        "before_bm": backward_maps.read().unwrap().stats());
    {
        trace_scoped!(
            "write_lock",
            "viewstr": view_string,
            "namespace": namespace,
            "br_path": br_path
        );
        let mut forward_maps = forward_maps.write().unwrap();
        let mut backward_maps = backward_maps.write().unwrap();
        forward_maps.merge(&fm);
        backward_maps.merge(&bm);
    }

    trace_end!(
        "merge_maps",
        "after_fm": forward_maps.read().unwrap().stats(),
        "after_bm": backward_maps.read().unwrap().stats());

    br_path.to_owned()
}

fn get_info(
    view_string: &str,
    prefix: &str,
    rev: &str,
    br_path: &Path,
    forward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
) -> String {
    trace_scoped!("get_info", "view_string": view_string, "br_path": br_path);

    let scratch = scratch::new(&br_path);

    let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone());
    let mut fm = view_maps::ViewMaps::new_downstream(forward_maps.clone());

    let viewobj = josh::build_view(&scratch, &view_string);

    let fr = &format!("refs/namespaces/{}/{}", &to_ns(&prefix), &rev);

    let obj = ok_or!(scratch.revparse_single(&fr), {
        return format!("rev not found: {:?}", &rev);
    });

    let commit = ok_or!(obj.peel_to_commit(), {
        return format!("not a commit");
    });

    let mut meta = HashMap::new();
    meta.insert("sha1".to_owned(), "".to_owned());
    let transformed = viewobj.apply_view_to_commit(&scratch, &commit, &mut fm, &mut bm, &mut meta);

    let transformed = scratch.find_commit(transformed).unwrap();

    let parent_ids = |commit: git2::Oid| {
        let pids: Vec<_> = scratch
            .find_commit(commit)
            .unwrap()
            .parent_ids()
            .map(|x| {
                json!({
                    "commit": x.to_string(),
                    "tree": scratch.find_commit(x).unwrap().tree_id().to_string(),
                })
            })
            .collect();
        pids
    };

    let s = json!({
        "original": {
            "commit": commit.id().to_string(),
            "tree": commit.tree_id().to_string(),
            "parents": parent_ids(commit.id()),
        },
        "transformed": {
            "commit": transformed.id().to_string(),
            "tree": transformed.tree_id().to_string(),
            "parents": parent_ids(transformed.id()),
        }
    });

    return serde_json::to_string(&s).unwrap();
}

fn install_josh_hook(scratch_dir: &Path) {
    if !scratch_dir.join("hooks/update").exists() {
        let shell = shell::Shell {
            cwd: scratch_dir.to_path_buf(),
        };
        shell.command("git config http.receivepack true");
        let ce = current_exe().expect("can't find path to exe");
        shell.command("rm -Rf hooks");
        shell.command("mkdir hooks");
        symlink(ce, scratch_dir.join("hooks").join("update")).expect("can't symlink update hook");
    }
}

fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    if args[0].ends_with("/update") {
        println!("josh-proxy");
        exit(virtual_repo::update_hook(&args[1], &args[2], &args[3]));
    }
    exit(run_proxy(args));
}
