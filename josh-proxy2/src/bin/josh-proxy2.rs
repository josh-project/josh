#[macro_use]
extern crate lazy_static;
use base64;

use futures::future;
use futures::FutureExt;
use futures::TryStreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server};
use std::collections::HashMap;
use std::env;
use std::net;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::process::Command;

fn version_str() -> String {
    format!(
        "Version: {}\n",
        option_env!("GIT_DESCRIBE").unwrap_or(std::env!("CARGO_PKG_VERSION"))
    )
}

lazy_static! {
    static ref ARGS: clap::ArgMatches<'static> = parse_args();
}

josh::regex_parsed!(
    TransformedRepoUrl,
    r"(?P<upstream_repo>/.*[.]git)(?P<headref>@[^:!]*)?(?P<view>[:!].*)[.](?P<ending>(?:git)|(?:json))(?P<pathinfo>/.*)?",
    [upstream_repo, view, pathinfo, headref, ending]
);

type CredentialCache = HashMap<String, std::time::Instant>;

#[derive(Clone)]
struct JoshProxyService {
    /* fetch_push_pool: futures_cpupool::CpuPool, */
    /* compute_pool: futures_cpupool::CpuPool, */
    port: String,
    repo_path: std::path::PathBuf,
    /* gerrit: Arc<josh_proxy::gerrit::Gerrit>, */
    upstream_url: String,
    forward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    fetching: Arc<RwLock<std::collections::HashSet<String>>>,
}

pub struct ServeTestGit {
    repo_path: PathBuf,
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
    let u = josh::ok_or!(String::from_utf8(line[6..].to_vec()), {
        return None;
    });
    let decoded = josh::ok_or!(base64::decode(&u), {
        return None;
    });
    let s = josh::ok_or!(String::from_utf8(decoded), {
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

async fn static_paths(
    service: &JoshProxyService,
    path: &str,
) -> Option<Response<hyper::Body>> {
    if path == "/version" {
        return Some(
            Response::builder()
                .status(hyper::StatusCode::OK)
                .body(hyper::Body::from(version_str()))
                .unwrap_or(Response::default()),
        );
    }
    if path == "/flush" {
        service.credential_cache.write().unwrap().clear();
        return Some(
            Response::builder()
                .status(hyper::StatusCode::OK)
                .body(hyper::Body::from("Flushed credential cache\n"))
                .unwrap_or(Response::default()),
        );
    }
    if path == "/views" {
        service.credential_cache.write().unwrap().clear();

        let repo = git2::Repository::init_bare(&service.repo_path).unwrap();
        let body_str = tokio::task::spawn_blocking(move || {
            let known_filters =
                josh::housekeeping::discover_filter_candidates(&repo).ok();
            toml::to_string_pretty(&known_filters).unwrap()
        })
        .await
        .unwrap();

        return Some(
            Response::builder()
                .status(hyper::StatusCode::OK)
                .body(hyper::Body::from(body_str))
                .unwrap_or(Response::default()),
        );
    }
    return None;
}

async fn repo_update_fn(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
) -> Response<hyper::Body> {
    let forward_maps = serv.forward_maps.clone();
    let backward_maps = serv.backward_maps.clone();

    let body = hyper::body::to_bytes(req.into_body()).await;

    let result = tokio::task::spawn_blocking(move || {
        let body = body?;
        let buffer = std::str::from_utf8(&body)?;
        josh_proxy2::process_repo_update(
            serde_json::from_str(&buffer)?,
            forward_maps,
            backward_maps,
        )
    })
    .await
    .unwrap();

    return match result {
        Ok(stderr) => Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(stderr)),
        Err(josh::JoshError(stderr)) => Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(hyper::Body::from(stderr)),
    }
    .unwrap();
}

async fn call_service(
    serv: Arc<JoshProxyService>,
    req: Request<hyper::Body>,
) -> Response<hyper::Body> {
    let repo = git2::Repository::init_bare(&serv.repo_path).unwrap();

    println!("call_service");

    let path = {
        let mut path = req.uri().path().to_owned();
        while path.contains("//") {
            path = path.replace("//", "/");
        }
        path
    };

    if let Some(r) = static_paths(&serv, &path).await {
        return r;
    }

    if path == "/repo_update" {
        return repo_update_fn(serv, req).await;
    }

    println!("ServeTestGit CALLING git-http backend");
    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&serv.repo_path);
    cmd.env("GIT_PROJECT_ROOT", &serv.repo_path);
    /* cmd.env("PATH_TRANSLATED", "/"); */
    cmd.env("GIT_DIR", &serv.repo_path);
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.env("PATH_INFO", &path);

    return hyper_cgi::do_cgi(req, cmd).await.0;

    /* /1* if path.starts_with("/review/") || path.starts_with("/c/") { *1/ */
    /* /1*     return service.gerrit.handle_request(req); *1/ */
    /* /1* } *1/ */

    /* let parsed_url = { */
    /*     let nop_path = path.replacen(".git", ".git:nop=nop.git", 1); */
    /*     if let Some(parsed_url) = TransformedRepoUrl::from_str(&path) { */
    /*         parsed_url */
    /*     } else if let Some(parsed_url) = TransformedRepoUrl::from_str(&nop_path) */
    /*     { */
    /*         parsed_url */
    /*     } else { */
    /*         return Box::new(futures::future::ok( */
    /*             Response::new().with_status(hyper::StatusCode::NotFound), */
    /*         )); */
    /*     } */
    /* }; */

    /* let headref = parsed_url.headref.trim_start_matches("@").to_owned(); */

    /* let compute_pool = service.compute_pool.clone(); */

    /* if parsed_url.ending == "json" { */
    /*     let forward_maps = service.forward_maps.clone(); */
    /*     let backward_maps = service.forward_maps.clone(); */

    /*     let f = compute_pool.spawn_fn(move || { */
    /*         let info = josh::housekeeping::get_info( */
    /*             &repo, */
    /*             &*josh::filters::parse(&parsed_url.view), */
    /*             &parsed_url.upstream_repo, */
    /*             &headref, */
    /*             forward_maps.clone(), */
    /*             backward_maps.clone(), */
    /*         ) */
    /*         .unwrap_or("get_info: error".to_owned()); */

    /*         let response = Response::new() */
    /*             .with_body(format!("{}\n", info)) */
    /*             .with_status(hyper::StatusCode::Ok); */
    /*         return Box::new(futures::future::ok(response)); */
    /*     }); */

    /*     return Box::new(f); */
    /* } */

    /* let (username, password) = josh::some_or!(josh_proxy::parse_auth(&req), { */
    /*     return Box::new(futures::future::ok( */
    /*         josh_proxy::respond_unauthorized(), */
    /*     )); */
    /* }); */

    /* let port = service.port.clone(); */

    /* let remote_url = [ */
    /*     service.upstream_url.as_str(), */
    /*     parsed_url.upstream_repo.as_str(), */
    /* ] */
    /* .join(""); */

    /* let br_url = remote_url.clone(); */
    /* let base_ns = josh::to_ns(&parsed_url.upstream_repo); */
    /* let handle = service.handle.clone(); */

    /* let fetch_future = if headref == "" { */
    /*     fetch_upstream( */
    /*         service.clone(), */
    /*         parsed_url.upstream_repo.clone(), */
    /*         &username, */
    /*         &password, */
    /*         br_url, */
    /*     ) */
    /* } else { */
    /*     fetch_upstream_ref( */
    /*         service.clone(), */
    /*         parsed_url.upstream_repo.clone(), */
    /*         &username, */
    /*         &password, */
    /*         br_url, */
    /*         headref.clone(), */
    /*     ) */
    /* }; */

    /* let temp_ns = */
    /*     Arc::new(josh_proxy::TmpGitNamespace::new(&service.repo_path)); */

    /* let refs = if headref != "" { */
    /*     Some(vec![( */
    /*         format!( */
    /*             "refs/josh/upstream/{}/{}", */
    /*             &josh::to_ns(&parsed_url.upstream_repo), */
    /*             headref */
    /*         ), */
    /*         temp_ns.reference("refs/heads/master"), */
    /*     )]) */
    /* } else { */
    /*     None */
    /* }; */

    /* let filter_spec = parsed_url.view.clone(); */
    /* let service = service.clone(); */
    /* let fs = filter_spec.clone(); */

    /* let ns = temp_ns.clone(); */

    /* let fetch_future = */
    /*     fetch_future.and_then(move |authorized| -> BoxedFuture<Response> { */
    /*         if !authorized { */
    /*             return Box::new(futures::future::ok( */
    /*                 josh_proxy::respond_unauthorized(), */
    /*             )); */
    /*         } */

    /*         let do_filter = do_filter( */
    /*             repo, */
    /*             &service, */
    /*             parsed_url.upstream_repo, */
    /*             ns.clone(), */
    /*             filter_spec, */
    /*             refs, */
    /*         ); */

    /*         let pathinfo = parsed_url.pathinfo.clone(); */
    /*         let mut cmd = std::process::Command::new("git"); */
    /*         let repo_path = service.repo_path.to_str().unwrap(); */
    /*         cmd.arg("http-backend"); */
    /*         cmd.current_dir(&service.repo_path); */
    /*         cmd.env("GIT_PROJECT_ROOT", repo_path); */
    /*         cmd.env("GIT_DIR", repo_path); */
    /*         cmd.env("GIT_HTTP_EXPORT_ALL", ""); */
    /*         cmd.env("PATH_INFO", pathinfo); */
    /*         cmd.env("JOSH_PASSWORD", password); */
    /*         cmd.env("JOSH_USERNAME", username); */
    /*         cmd.env("JOSH_PORT", port); */
    /*         cmd.env("GIT_NAMESPACE", ns.name().clone()); */
    /*         cmd.env("JOSH_VIEWSTR", fs); */
    /*         cmd.env("JOSH_REMOTE", remote_url); */
    /*         cmd.env("JOSH_BASE_NS", base_ns); */

    /*         let response = do_filter.and_then(move |_| { */
    /*             tracing::trace!("git-http backend {:?}", path); */
    /*             josh_proxy::do_cgi(req, cmd, handle.clone()) */
    /*         }); */

    /*         Box::new(response) */
    /*     }); */

    /* // This is chained as a seperate future to make sure that */
    /* // it is executed in all cases. */
    /* Box::new({ */
    /*     fetch_future.map(move |response| { */
    /*         std::mem::drop(temp_ns); */
    /*         response */
    /*     }) */
    /* }) */
}

async fn run_test_server() -> josh::JoshResult<i32> {
    let repo_path =
        PathBuf::from(ARGS.value_of("local").expect("missing local directory"));
    let port = ARGS.value_of("port").unwrap_or("8000").to_owned();
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let serve_test_git = Arc::new(ServeTestGit {
        repo_path: repo_path.to_owned(),
    });

    let remote = ARGS.value_of("remote").expect("missing remote host url");
    let local = std::path::PathBuf::from(
        ARGS.value_of("local").expect("missing local directory"),
    );

    josh_proxy2::create_repo(&local)?;

    let forward_maps = Arc::new(RwLock::new(josh::view_maps::try_load(
        &local.join("josh_forward_maps"),
    )));
    let backward_maps = Arc::new(RwLock::new(josh::view_maps::try_load(
        &local.join("josh_backward_maps"),
    )));

    let proxy_service = Arc::new(JoshProxyService {
        /* fetch_push_pool: futures_cpupool::CpuPool::new( */
        /*     ARGS.value_of("n") */
        /*         .unwrap_or("1") */
        /*         .parse() */
        /*         .expect("not a number"), */
        /* ), */
        /* compute_pool: futures_cpupool::CpuPool::new(4), */
        port: port,
        repo_path: local.to_owned(),
        forward_maps: forward_maps,
        backward_maps: backward_maps,
        upstream_url: remote.to_owned(),
        credential_cache: Arc::new(RwLock::new(CredentialCache::new())),
        fetching: Arc::new(RwLock::new(std::collections::HashSet::new())),
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

    server.await?;
    Ok(0)
}

fn parse_args() -> clap::ArgMatches<'static> {
    let args = {
        let mut args = vec![];
        for arg in std::env::args() {
            args.push(arg);
        }
        args
    };

    clap::App::new("josh-proxy")
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
        .arg(
            clap::Arg::with_name("gc")
                .long("gc")
                .takes_value(false)
                .help("Run git gc in maintanance"),
        )
        .arg(
            clap::Arg::with_name("m")
                .short("m")
                .takes_value(false)
                .help("Only run maintance and exit"),
        )
        .arg(
            clap::Arg::with_name("n").short("n").takes_value(true).help(
                "Number of concurrent upstream git fetch/push operations",
            ),
        )
        /* .arg( */
        /*     clap::Arg::with_name("g") */
        /*         .short("g") */
        /*         .takes_value(false) */
        /*         .help("Enable gerrit integration"), */
        /* ) */
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .get_matches_from(args)
}

fn update_hook(refname: &str, old: &str, new: &str) -> josh::JoshResult<i32> {
    let mut repo_update = HashMap::new();
    repo_update.insert("new".to_owned(), new.to_owned());
    repo_update.insert("old".to_owned(), old.to_owned());
    repo_update.insert("refname".to_owned(), refname.to_owned());

    for (env_name, name) in [
        ("JOSH_USERNAME", "username"),
        ("JOSH_PASSWORD", "password"),
        ("JOSH_REMOTE", "remote_url"),
        ("JOSH_BASE_NS", "base_ns"),
        ("JOSH_VIEWSTR", "filter_spec"),
        ("GIT_NAMESPACE", "GIT_NAMESPACE"),
    ]
    .iter()
    {
        repo_update.insert(name.to_string(), std::env::var(&env_name)?);
    }

    repo_update.insert(
        "GIT_DIR".to_owned(),
        git2::Repository::init_bare(&std::path::Path::new(&std::env::var(
            "GIT_DIR",
        )?))?
        .path()
        .to_str()
        .ok_or(josh::josh_error("GIT_DIR not set"))?
        .to_owned(),
    );

    let port = std::env::var("JOSH_PORT")?;

    let client = reqwest::Client::builder().timeout(None).build()?;
    let resp = client
        .post(&format!("http://localhost:{}/repo_update", port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(mut r) => {
            if let Ok(body) = r.text() {
                println!("response from upstream:\n {}\n\n", body);
            } else {
                println!("no upstream response");
            }
            if r.status().is_success() {
                return Ok(0);
            } else {
                return Ok(1);
            }
        }
        Err(err) => {
            tracing::warn!("/repo_update request failed {:?}", err);
        }
    };
    return Ok(1);
}

fn main() {
    // josh-proxy creates a symlink to itself as a git update hook.
    // When it gets called by git as that hook, the binary name will end
    // end in "/update" and this will not be a new server.
    // The update hook will then make a http request back to the main
    // process to do the actual computation while taking advantage of the
    // cached data already loaded into the main processe's memory.
    if let [a0, a1, a2, a3, ..] =
        &std::env::args().collect::<Vec<_>>().as_slice()
    {
        if a0.ends_with("/update") {
            println!("josh-proxy");
            std::process::exit(update_hook(&a1, &a2, &a3).unwrap_or(1));
        }
    }

    // As the tokio runtime shall only be started when we are not running as
    // a hook we cannot use the #[tokio::main] macro.
    // This is doing the same thing by hand.
    tokio::runtime::Builder::new_multi_thread()
        .build()
        .unwrap()
        .block_on(async {
            run_test_server().await;
        })
}
