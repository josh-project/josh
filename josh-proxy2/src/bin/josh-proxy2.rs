#[macro_use]
extern crate lazy_static;
use base64;

use futures::future;
use futures::FutureExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Server};
use std::collections::HashMap;
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
    port: String,
    repo_path: std::path::PathBuf,
    /* gerrit: Arc<josh_proxy::gerrit::Gerrit>, */
    upstream_url: String,
    forward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    credential_cache: Arc<RwLock<CredentialCache>>,
    fetching: Arc<RwLock<std::collections::HashSet<String>>>,
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

fn hash_strings(url: &str, username: &str, password: &str) -> String {
    use crypto::digest::Digest;
    let mut d = crypto::sha1::Sha1::new();
    d.input_str(&format!("{}:{}:{}", &url, &username, &password));
    d.result_str().to_owned()
}

async fn fetch_upstream(
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    username: &str,
    password: &str,
    remote_url: String,
) -> bool {
    let username = username.to_owned();
    let password = password.to_owned();
    let credentials_hashed = hash_strings(&remote_url, &username, &password);

    tracing::debug!(
        "credentials_hashed {:?}, {:?}, {:?}",
        &remote_url,
        &username,
        &credentials_hashed
    );

    let refs_to_fetch = vec!["refs/heads/*", "refs/tags/*"];

    //let credentials_cached_ok = {
    //    let last = service
    //        .credential_cache
    //        .read()
    //        .ok()
    //        .map(|cc| cc.get(&credentials_hashed).copied());

    //    if let Some(Some(c)) = last {
    //        std::time::Instant::now().duration_since(c)
    //            < std::time::Duration::from_secs(60)
    //    } else {
    //        false
    //    }
    //};

    //if credentials_cached_ok
    //    && !service
    //        .fetching
    //        .write()
    //        .map(|mut x| x.insert(credentials_hashed.clone()))
    //        .unwrap_or(true)
    //{
    //    return true;
    //}

    let credential_cache = service.credential_cache.clone();
    let br_path = service.repo_path.clone();
    let fetching = service.fetching.clone();

    let res = tokio::task::spawn_blocking(move || {
        josh_proxy2::fetch_refs_from_url(
            &br_path,
            &upstream_repo,
            &remote_url,
            &refs_to_fetch,
            &username,
            &password,
        )
    })
    .await
    .unwrap();

    if let Ok(_) = res {
        if let Ok(mut x) = fetching.write() {
            x.remove(&credentials_hashed);
        } else {
            tracing::error!("lock poisoned");
        }
        if let Ok(mut cc) = credential_cache.write() {
            cc.insert(credentials_hashed, std::time::Instant::now());
        } else {
            tracing::error!("lock poisoned");
        }
        return true;
    }

    return false;

    // TODO
    /* if credentials_cached_ok { */
    /*     do_fetch.forget(); */
    /*     return true; */
    /* } */

    /* return do_fetch; */
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

async fn do_filter(
    repo: git2::Repository,
    service: Arc<JoshProxyService>,
    upstream_repo: String,
    temp_ns: Arc<josh_proxy2::TmpGitNamespace>,
    filter_spec: String,
    from_to: Option<Vec<(String, String)>>,
) -> git2::Repository {
    let forward_maps = service.forward_maps.clone();
    let backward_maps = service.backward_maps.clone();
    return tokio::task::spawn_blocking(move || {
        let filter = josh::filters::parse(&filter_spec);
        let filter_spec = filter.filter_spec();
        let from_to = from_to.unwrap_or_else(|| {
            josh::housekeeping::default_from_to(
                &repo,
                &temp_ns.name(),
                &upstream_repo,
                &filter_spec,
            )
        });
        let mut bm = josh::view_maps::new_downstream(&backward_maps);
        let mut fm = josh::view_maps::new_downstream(&forward_maps);
        josh::scratch::apply_filter_to_refs(
            &repo, &*filter, &from_to, &mut fm, &mut bm,
        );
        josh::view_maps::try_merge_both(forward_maps, backward_maps, &fm, &bm);
        repo.reference_symbolic(
            &temp_ns.reference("HEAD"),
            &temp_ns.reference("refs/heads/master"),
            true,
            "",
        )
        .ok();
        return repo;
    })
    .await
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

    /* /1* if path.starts_with("/review/") || path.starts_with("/c/") { *1/ */
    /* /1*     return service.gerrit.handle_request(req); *1/ */
    /* /1* } *1/ */

    let parsed_url = {
        let nop_path = path.replacen(".git", ".git:nop=nop.git", 1);
        if let Some(parsed_url) = TransformedRepoUrl::from_str(&path) {
            parsed_url
        } else if let Some(parsed_url) = TransformedRepoUrl::from_str(&nop_path)
        {
            parsed_url
        } else {
            return Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .body(hyper::Body::empty())
                .unwrap();
        }
    };

    let headref = parsed_url.headref.trim_start_matches("@").to_owned();

    if parsed_url.ending == "json" {
        let forward_maps = serv.forward_maps.clone();
        let backward_maps = serv.forward_maps.clone();

        let info_str = tokio::task::spawn_blocking(move || {
            josh::housekeeping::get_info(
                &repo,
                &*josh::filters::parse(&parsed_url.view),
                &parsed_url.upstream_repo,
                &headref,
                forward_maps.clone(),
                backward_maps.clone(),
            )
            .unwrap_or("get_info: error".to_owned())
        })
        .await
        .unwrap();

        return Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(format!("{}\n", info_str)))
            .unwrap();
    }

    let (username, password) = josh::some_or!(parse_auth(&req), {
        let builder = Response::builder()
            .header("WWW-Authenticate", "Basic realm=User Visible Realm")
            .status(hyper::StatusCode::UNAUTHORIZED);
        return builder.body(hyper::Body::empty()).unwrap();
    });

    let port = serv.port.clone();

    let remote_url = [
        serv.upstream_url.as_str(),
        parsed_url.upstream_repo.as_str(),
    ]
    .join("");

    let br_url = remote_url.clone();
    let base_ns = josh::to_ns(&parsed_url.upstream_repo);

    /* if headref == "" { */
    let authorized = fetch_upstream(
        serv.clone(),
        parsed_url.upstream_repo.clone(),
        &username,
        &password,
        br_url,
    )
    .await;
    /* } else { */
    // TODO
    /*     fetch_upstream_ref( */
    /*         service.clone(), */
    /*         parsed_url.upstream_repo.clone(), */
    /*         &username, */
    /*         &password, */
    /*         br_url, */
    /*         headref.clone(), */
    /*     ).await */
    /* }; */

    if !authorized {
        let builder = Response::builder()
            .header("WWW-Authenticate", "Basic realm=User Visible Realm")
            .status(hyper::StatusCode::UNAUTHORIZED);
        return builder.body(hyper::Body::empty()).unwrap();
    }

    let temp_ns = Arc::new(josh_proxy2::TmpGitNamespace::new(&serv.repo_path));

    let refs = if headref != "" {
        Some(vec![(
            format!(
                "refs/josh/upstream/{}/{}",
                &josh::to_ns(&parsed_url.upstream_repo),
                headref
            ),
            temp_ns.reference("refs/heads/master"),
        )])
    } else {
        None
    };

    let filter_spec = parsed_url.view.clone();
    let serv = serv.clone();
    let fs = filter_spec.clone();

    let ns = temp_ns.clone();

    do_filter(
        repo,
        serv.clone(),
        parsed_url.upstream_repo,
        ns.clone(),
        filter_spec,
        refs,
    )
    .await;

    let pathinfo = parsed_url.pathinfo.clone();
    let repo_path = serv.repo_path.to_str().unwrap();

    println!("Josh Proxy CALLING git-http backend");
    let mut cmd = Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&serv.repo_path);
    cmd.env("GIT_DIR", repo_path);
    cmd.env("GIT_HTTP_EXPORT_ALL", "");
    cmd.env("GIT_NAMESPACE", ns.name().clone());
    cmd.env("GIT_PROJECT_ROOT", repo_path);
    cmd.env("JOSH_BASE_NS", base_ns);
    cmd.env("JOSH_PASSWORD", password);
    cmd.env("JOSH_PORT", port);
    cmd.env("JOSH_REMOTE", remote_url);
    cmd.env("JOSH_USERNAME", username);
    cmd.env("JOSH_VIEWSTR", fs);
    cmd.env("PATH_INFO", pathinfo);

    let cgires = hyper_cgi::do_cgi(req, cmd).await.0;

    // This is chained as a seperate future to make sure that
    // it is executed in all cases.
    std::mem::drop(temp_ns);

    return cgires;
}

#[tokio::main]
async fn run_proxy() -> josh::JoshResult<i32> {
    let port = ARGS.value_of("port").unwrap_or("8000").to_owned();
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();

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
        port: port,
        repo_path: local.to_owned(),
        forward_maps: forward_maps,
        backward_maps: backward_maps,
        upstream_url: remote.to_owned(),
        credential_cache: Arc::new(RwLock::new(CredentialCache::new())),
        fetching: Arc::new(RwLock::new(std::collections::HashSet::new())),
    });

    let make_service = make_service_fn(move |_| {
        let proxy_service = proxy_service.clone();

        let service = service_fn(move |_req| {
            let proxy_service = proxy_service.clone();

            call_service(proxy_service, _req).map(Ok::<_, hyper::http::Error>)
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

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let resp = client
        .post(&format!("http://localhost:{}/repo_update", port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(r) => {
            let success = r.status().is_success();
            if let Ok(body) = r.text() {
                println!("response from upstream:\n {}\n\n", body);
            } else {
                println!("no upstream response");
            }
            if success {
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

    std::process::exit(run_proxy().unwrap_or(1));
}
