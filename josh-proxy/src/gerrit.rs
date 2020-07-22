josh::regex_parsed!(ChangeUrl, r"/c/(?P<change>.*)/", [change]);

type HttpClient =
    hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;
use hyper::server::{Request, Response};

use super::BoxedFuture;
use futures::future::Future;
use futures::Stream;
use std::str::FromStr;

pub struct Gerrit {
    handle: tokio_core::reactor::Handle,
    repo_path: std::path::PathBuf,
    http_client: HttpClient,
    upstream_url: String,
    cpu_pool: futures_cpupool::CpuPool,
}

fn j2str(val: &serde_json::Value, s: &str) -> String {
    if let Some(r) = val.pointer(s) {
        return r.to_string().trim_matches('"').to_string();
    }
    return format!("## not found: {:?}", s);
}

impl Gerrit {
    pub fn new(
        core: &tokio_core::reactor::Core,
        repo_path: std::path::PathBuf,
        upstream_url: String,
    ) -> Gerrit {
        Gerrit {
            handle: core.handle(),
            repo_path: repo_path,
            http_client: hyper::Client::configure()
                .connector(
                    ::hyper_tls::HttpsConnector::new(4, &core.handle())
                        .unwrap(),
                )
                .keep_alive(true)
                .build(&core.handle()),
            upstream_url: upstream_url,
            cpu_pool: futures_cpupool::CpuPool::new(4),
        }
    }
    pub fn handle_request(&self, req: Request) -> BoxedFuture<Response> {
        let (username, password) = josh::some_or!(super::parse_auth(&req), {
            return Box::new(
                futures::future::ok(super::respond_unauthorized()),
            );
        });
        let mut req = req;
        tracing::info!("gerrit handle_request static {:?}", &req.path());
        let mut is_static = req.path().starts_with("/review/static/")
            || req.path().starts_with("/review/pkg/");

        if !is_static && req.path().starts_with("/review/") {
            req.set_uri(
                hyper::Uri::from_str("/review/static/index.html").unwrap(),
            );
            is_static = true;
        }
        if is_static {
            tracing::info!("serving static {:?}", &req.path());
            return Box::new(
                hyper_fs::StaticFs::new(
                    self.handle.clone(),
                    self.cpu_pool.clone(),
                    "/review/",
                    "./josh-review/",
                    hyper_fs::Config::new(),
                )
                .call(req)
                .or_else(hyper_fs::error_handler)
                .map(|res_req| res_req.0),
            );
        }

        if let Some(parsed_url) = ChangeUrl::from_str(&req.path()) {
            return self.change_page(&username, &password, &parsed_url.change);
        };

        if req.path() == "/review/" {
            return self.list_page(&username, &password);
        };

        return Box::new(futures::future::ok(
            Response::new().with_status(hyper::StatusCode::NotFound),
        ));
    }

    fn gerrit_api(
        &self,
        username: &str,
        password: &str,
        endpoint: &str,
        query: String,
    ) -> BoxedFuture<serde_json::Value> {
        let uri = hyper::Uri::from_str(&format!(
            "{}{}?{}",
            self.upstream_url, endpoint, query
        ))
        .unwrap();

        println!("gerrit_api: {:?}", &uri);

        let auth = hyper::header::Authorization(hyper::header::Basic {
            username: username.to_string(),
            password: Some(password.to_string()),
        });

        let mut r = Request::new(hyper::Method::Get, uri);
        r.headers_mut().set(auth);
        return Box::new(
            self.http_client
                .request(r)
                .and_then(move |x| x.body().concat2().map(super::body2string))
                .and_then(move |resp_text| {
                    println!("gerrit_api resp: {}", &resp_text);
                    if resp_text.len() < 4 {
                        return futures::future::ok("to short".into());
                    }
                    let v: serde_json::Value =
                        serde_json::from_str(&resp_text[4..])
                            .unwrap_or("can't parse json".into());
                    futures::future::ok(v)
                }),
        );
    }

    fn change_page(
        &self,
        username: &str,
        password: &str,
        change: &str,
    ) -> BoxedFuture<Response> {
        let get_comments = self.gerrit_api(
            &username,
            &password,
            &format!("/a/changes/{}/comments", &change),
            format!(""),
        );
        let get_changes = self.gerrit_api(
            &username,
            &password,
            "/a/changes/",
            format!("q=change:{}&o=ALL_REVISIONS&o=ALL_COMMITS", change),
        );

        let cpu_pool = self.cpu_pool.clone();
        let br_path = self.repo_path.clone();
        let r = get_changes
            .and_then(move |change_json| {
                let to = j2str(&change_json, "/0/current_revision");
                let from = j2str(
                    &change_json,
                    &format!("/0/revisions/{}/commit/parents/0/commit", &to),
                );
                let cmd = format!("git diff -U99999999 {}..{}", from, to);

                println!("diffcmd: {:?}", cmd);
                git_command(cmd, br_path, cpu_pool)
                    .join(futures::future::ok(change_json))
            })
            .and_then(move |(stdout, change_json)| {
                let mut resp =
                    std::collections::HashMap::<String, String>::new();
                resp.insert("diff".to_owned(), stdout);
                futures::future::ok((resp, change_json))
            })
            .join(get_comments)
            .and_then(move |((resp, change_json), comments_value)| {
                let mut revision2sha =
                    std::collections::HashMap::<i64, String>::new();
                for (k, v) in
                    change_json[0]["revisions"].as_object().unwrap().iter()
                {
                    revision2sha
                        .insert(v["_number"].as_i64().unwrap(), k.to_string());
                }

                for i in comments_value.as_object().unwrap().keys() {
                    println!("comments_value: {:?}", &i);
                }

                let response = hyper::server::Response::new()
                    .with_body(serde_json::to_string(&resp).unwrap())
                    .with_status(hyper::StatusCode::Ok);
                futures::future::ok(response)
            });
        return Box::new(r);
    }

    fn list_page(
        &self,
        username: &str,
        password: &str,
    ) -> BoxedFuture<Response> {
        let get_changes = self.gerrit_api(
            &username,
            &password,
            "/a/changes/",
            format!("o=ALL_REVISIONS&o=ALL_COMMITS"),
        );

        let r = get_changes.and_then(move |change_json| {
            let mut revision2sha =
                std::collections::HashMap::<i64, String>::new();
            for (k, v) in
                change_json[0]["revisions"].as_object().unwrap().iter()
            {
                revision2sha
                    .insert(v["_number"].as_i64().unwrap(), k.to_string());
            }

            let response = hyper::server::Response::new()
                .with_body(serde_json::to_string(&change_json).unwrap())
                .with_status(hyper::StatusCode::Ok);
            futures::future::ok(response)
        });
        return Box::new(r);
    }
}

fn git_command(
    cmd: String,
    br_path: std::path::PathBuf,
    cpu_pool: futures_cpupool::CpuPool,
) -> BoxedFuture<String> {
    return Box::new(cpu_pool.spawn_fn(move || {
        let shell = josh::shell::Shell {
            cwd: br_path.to_owned(),
        };
        let (stdout, _stderr) = shell.command(&cmd);
        return futures::future::ok(stdout);
    }));
}

pub fn spawn_poll_thread(
    repo_path: std::path::PathBuf,
    remote_url: String,
) -> std::thread::JoinHandle<()> {
    let username = std::env::var("JOSH_GERRIT_USERNAME")
        .expect("JOSH_GERRIT_USERNAME needs to be set");
    let password = std::env::var("JOSH_GERRIT_PASSWORD")
        .expect("JOSH_GERRIT_PASSWORD needs to be set");
    std::thread::spawn(move || loop {
        let repo = git2::Repository::init_bare(&repo_path).unwrap();

        for upstream_repo in josh::housekeeping::discover_repos(&repo)
            .expect("discover_repos fail")
        {
            let rurl = format!("{}/{}", &remote_url, &upstream_repo);
            super::fetch_refs_from_url(
                &repo_path,
                &upstream_repo,
                &rurl,
                &["refs/changes/*"],
                &username,
                &password,
            )
            .ok();
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    })
}
