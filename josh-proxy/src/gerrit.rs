josh::regex_parsed!(ChangeUrl, r"/c/(?P<change>.*)/", [change]);

type HttpClient =
    hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;
use hyper::server::{Request, Response};

use super::BoxedFuture;
use futures::future::Future;
use futures::Stream;
use std::str::FromStr;

pub struct Gerrit {
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
            repo_path: repo_path,
            http_client: hyper::Client::configure()
                .connector(
                    ::hyper_tls::HttpsConnector::new(4, &core.handle())
                        .unwrap(),
                )
                .keep_alive(true)
                .build(&core.handle()),
            upstream_url: upstream_url,
            cpu_pool: futures_cpupool::CpuPool::new(1),
        }
    }
    pub fn handle_request(&self, path: &str) -> Option<BoxedFuture<Response>> {
        if path.starts_with("/static/") {
            return Some(Box::new(
                self.http_client.get(
                    hyper::Uri::from_str(&format!(
                        "http://localhost:3000{}",
                        &path
                    ))
                    .unwrap(),
                ),
            ));
        }
        if path.starts_with("/_c/") {
            return Some(Box::new(
                self.http_client.get(
                    hyper::Uri::from_str("http://localhost:3000/index.html")
                        .unwrap(),
                ),
            ));
        }

        let parsed_url =
            josh::some_or!(ChangeUrl::from_str(&path), { return None });

        let pool = self.cpu_pool.clone();

        let get_comments = self.gerrit_api(
            &format!("/a/changes/{}/comments", parsed_url.change),
            format!(""),
        );

        let br_path = self.repo_path.clone();
        let r = self
            .gerrit_api(
                "/a/changes/",
                format!(
                    "q=change:{}&o=ALL_REVISIONS&o=ALL_COMMITS",
                    parsed_url.change
                ),
            )
            .and_then(move |change_json| {
                let to = j2str(&change_json, "/0/current_revision");
                let from = j2str(
                    &change_json,
                    &format!("/0/revisions/{}/commit/parents/0/commit", &to),
                );
                let mut resp =
                    std::collections::HashMap::<String, String>::new();
                let cmd = format!("git diff -U99999999 {}..{}", from, to);
                println!("diffcmd: {:?}", cmd);
                git_command(cmd, br_path.to_owned(), pool.clone()).and_then(
                    move |stdout| {
                        resp.insert("diff".to_owned(), stdout);
                        futures::future::ok((resp, change_json))
                    },
                )
            })
            .and_then(move |(resp, change_json)| {
                let mut revision2sha =
                    std::collections::HashMap::<i64, String>::new();
                for (k, v) in
                    change_json[0]["revisions"].as_object().unwrap().iter()
                {
                    revision2sha
                        .insert(v["_number"].as_i64().unwrap(), k.to_string());
                }

                get_comments.and_then(move |comments_value| {
                    for i in comments_value.as_object().unwrap().keys() {
                        println!("comments_value: {:?}", &i);
                    }

                    let response = hyper::server::Response::new()
                        .with_body(serde_json::to_string(&resp).unwrap())
                        .with_status(hyper::StatusCode::Ok);
                    futures::future::ok(response)
                })
            });

        return Some(Box::new(r));
    }

    fn gerrit_api(
        &self,
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
            username: std::env::var("JOSH_USERNAME").unwrap_or("".to_owned()),
            password: std::env::var("JOSH_PASSWORD").ok(),
        });

        let mut r = Request::new(hyper::Method::Get, uri);
        r.headers_mut().set(auth);
        return Box::new(
            self.http_client
                .request(r)
                .and_then(move |x| x.body().concat2().map(super::body2string))
                .and_then(move |resp_text| {
                    println!("gerrit_api resp: {}", &resp_text);
                    let v: serde_json::Value =
                        serde_json::from_str(&resp_text[4..]).unwrap();
                    futures::future::ok(v)
                }),
        );
    }
}
fn git_command(
    cmd: String,
    br_path: std::path::PathBuf,
    pool: futures_cpupool::CpuPool,
) -> BoxedFuture<String> {
    return Box::new(pool.spawn_fn(move || {
        let shell = josh::shell::Shell {
            cwd: br_path.to_owned(),
        };
        let (stdout, _stderr) = shell.command(&cmd);
        return futures::future::ok(stdout);
    }));
}
