use anyhow::{Context, anyhow, ensure};
use axum::response::IntoResponse;
use axum::{Router, extract::Request, response::Response, routing::any};
use josh_proxy::TmpGitNamespace;
use josh_proxy::service::{JoshProxyService, make_service, make_service_router};
use rand::prelude::*;
use reqwest::StatusCode;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

const N_FILES: usize = 1_000;
const NESTING_LEVEL: usize = 3;
const N_PER_SUBFOLDER_MIN: usize = 5;
const N_PER_SUBFOLDER_MAX: usize = 20;
const PATH_COMPONENT_LENGTH: usize = 15;
const ROOT: &str = "root";

struct FileFixture {
    path: String,
    content: String,
}

struct AbortTasks(Vec<tokio::task::JoinHandle<()>>);

impl Drop for AbortTasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

fn random_string(rng: &mut StdRng, len: usize) -> String {
    (0..len)
        .map(|_| {
            use rand::distr::Alphabetic;
            (Alphabetic.sample(rng) as char).to_ascii_lowercase()
        })
        .collect()
}

fn build_upstream(path: &Path) -> anyhow::Result<(String, Vec<FileFixture>)> {
    let repo = gix::init_bare(path)?;
    let mut rng = StdRng::seed_from_u64(0);
    let files_in_folder =
        rand::distr::Uniform::try_from(N_PER_SUBFOLDER_MIN..=N_PER_SUBFOLDER_MAX)?;
    let empty_tree = repo.write_object(gix::objs::Tree { entries: vec![] })?;
    let mut editor = repo.edit_tree(empty_tree)?;
    let mut files = Vec::with_capacity(N_FILES);
    let mut paths = std::collections::BTreeSet::new();

    while files.len() < N_FILES {
        let mut subpath = PathBuf::from(ROOT);
        subpath.extend((0..NESTING_LEVEL).map(|_| random_string(&mut rng, PATH_COMPONENT_LENGTH)));
        let to_add = files_in_folder.sample(&mut rng).min(N_FILES - files.len());

        for i in 0..to_add {
            let path = subpath.join(format!("file_{i}"));
            let path = path.to_string_lossy().replace('\\', "/");
            if !paths.insert(path.clone()) {
                continue;
            }
            let content = format!("file {}: {}\n", files.len(), random_string(&mut rng, 32));
            let blob = repo.write_blob(content.as_bytes())?;
            editor.upsert(&path, gix::objs::tree::EntryKind::Blob, blob)?;
            files.push(FileFixture { path, content });
        }
    }

    let tree = editor.write()?.detach();
    let signature = gix::actor::Signature {
        name: "Josh proxy integration test".into(),
        email: "josh-proxy@example.com".into(),
        time: gix::actor::date::Time {
            seconds: 1_700_000_000,
            offset: 0,
        },
    };
    let mut committer_time = gix::date::parse::TimeBuf::default();
    let mut author_time = gix::date::parse::TimeBuf::default();
    let head = repo
        .commit_as(
            signature.to_ref(&mut committer_time),
            signature.to_ref(&mut author_time),
            "refs/heads/master",
            "deterministic wide tree",
            tree,
            std::iter::empty::<gix::ObjectId>(),
        )?
        .detach();

    Ok((head.to_string(), files))
}

async fn git_handler(upstream_dir: PathBuf, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let git_path = path.strip_prefix("/repo.git").unwrap_or(&path);
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&upstream_dir);
    cmd.env("GIT_PROJECT_ROOT", &upstream_dir);
    cmd.env("GIT_HTTP_EXPORT_ALL", "1");
    cmd.env("PATH_INFO", git_path);

    let (response, stream) = match axum_cgi::do_cgi(req, cmd).await {
        Ok((response, stream)) => (response, stream),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };

    response
        .body(axum::body::Body::from_stream(stream))
        .expect("failed to build upstream response")
        .into_response()
}

async fn exercise_graphql(
    client: &reqwest::Client,
    proxy_url: &str,
    service: &Arc<JoshProxyService>,
    diagnostic_repo: &gix::Repository,
    head: &str,
    files: &[FileFixture],
) -> anyhow::Result<()> {
    let mut deferred_flush_error = None;
    let mut previous_oid: Option<gix::ObjectId> = None;
    let mut previous_tree_oid: Option<gix::ObjectId> = None;

    for (index, file) in files.iter().enumerate() {
        let query_number = index + 1;
        if let Some(oid) = previous_oid {
            if let Err(error) = diagnostic_repo.find_object(oid) {
                eprintln!("overlay diagnostic for GraphQL query {query_number} failed: {error:?}");
            }
        }
        let previous_tree_filter = previous_tree_oid.map(|oid| format!(":$.={oid}"));
        let response = client
            .post(proxy_url)
            .json(&serde_json::json!({
                "query": "query File($at: String!, $filter: String!, $previousTreeFilter: String, $path: String!) { previous: rev(at: $at, filter: $previousTreeFilter) { dir { hash } } current: rev(at: $at, filter: $filter) { hash dir { hash } file(path: $path) { text } } }",
                "variables": {
                    "at": head,
                    "filter": format!("::{}", file.path),
                    "previousTreeFilter": previous_tree_filter,
                    "path": file.path,
                },
            }))
            .send()
            .await
            .with_context(|| format!("GraphQL query {query_number} failed to send"))?;
        let status = response.status();
        let body = response.text().await?;
        ensure!(
            status.is_success(),
            "GraphQL query {query_number} for {} returned HTTP {status}: {body}",
            file.path
        );

        let json: Value = serde_json::from_str(&body)
            .with_context(|| format!("GraphQL query {query_number} returned invalid JSON"))?;
        ensure!(
            json.get("errors").is_none(),
            "GraphQL query {query_number} for {} failed: {body}",
            file.path
        );
        let actual = json
            .pointer("/data/current/file/text")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow!(
                    "GraphQL query {query_number} for {} returned no file text: {body}",
                    file.path
                )
            })?;
        ensure!(
            actual == file.content,
            "GraphQL query {query_number} for {} returned incorrect content: {body}",
            file.path
        );
        if let Some(expected) = previous_tree_oid {
            let previous_hash = json
                .pointer("/data/previous/dir/hash")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("GraphQL query {query_number} returned no previous tree hash: {body}")
                })?;
            ensure!(
                previous_hash == expected.to_string(),
                "previous tree filter was not applied: {body}"
            );
        }
        previous_tree_oid = Some(gix::ObjectId::from_str(
            json.pointer("/data/current/dir/hash")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("GraphQL query {query_number} returned no current tree hash: {body}")
                })?,
        )?);
        previous_oid = Some(gix::ObjectId::from_str(
            json.pointer("/data/current/hash")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("GraphQL query {query_number} returned no current commit hash: {body}")
                })?,
        )?);

        // Persist each query's objects so the next request has one more overlay pack to discover.
        if let Err(error) = service.open_overlay(None)?.flush_mem_odb() {
            let error = error.context(format!(
                "flushing overlay objects created by GraphQL query {query_number}"
            ));
            eprintln!("{error:#}");
            deferred_flush_error.get_or_insert(error);
        }

        if query_number % 100 == 0 {
            let pack_count = std::fs::read_dir(service.repo_path.join("overlay/objects/pack"))?
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "pack"))
                .count();
            eprintln!(
                "completed {query_number}/{N_FILES} GraphQL file queries ({pack_count} overlay packs)"
            );
        }
    }

    if let Some(error) = deferred_flush_error {
        return Err(error);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "reproduces gitoxide InsufficientSlots as GraphQL overlay packs accumulate"]
async fn graphql_file_queries_survive_overlay_pack_growth() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let upstream_dir = temp_dir.path().join("upstream");
    let proxy_dir = temp_dir.path().join("proxy");
    let (head, files) = build_upstream(&upstream_dir)?;
    ensure!(
        files.len() == N_FILES,
        "fixture must contain {N_FILES} files"
    );

    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let upstream_port = upstream_listener.local_addr()?.port();
    let upstream_app = Router::new().route(
        "/{*path}",
        any({
            let upstream_dir = upstream_dir.clone();
            move |req| git_handler(upstream_dir.clone(), req)
        }),
    );
    let upstream_task = tokio::spawn(async move {
        axum::serve(upstream_listener, upstream_app)
            .await
            .expect("upstream server error");
    });
    let mut server_tasks = AbortTasks(vec![upstream_task]);

    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let proxy_port = proxy_listener.local_addr()?.port();
    josh_proxy::service::create_repo(&proxy_dir, None)?;
    let diagnostic_repo = gix::open(proxy_dir.join("overlay"))?;
    josh_core::cache::SledCacheBackend::new(&proxy_dir).pin()?;
    let (io_tx, mut io_rx) = tokio::sync::mpsc::unbounded_channel();
    let io_task = tokio::task::spawn_blocking(move || {
        while let Some(josh_proxy::service::IoCleanup { repo_path, name }) = io_rx.blocking_recv() {
            TmpGitNamespace::cleanup(&repo_path, &name);
        }
    });
    let service = make_service()
        .port(proxy_port)
        .repo_path(&proxy_dir)
        .remotes(&[josh_proxy::cli::Remote::Http(format!(
            "http://127.0.0.1:{upstream_port}"
        ))])
        .io_thread_tx(io_tx)
        .call()?;
    let proxy_service = service.clone();
    let proxy_task = tokio::spawn(async move {
        axum::serve(proxy_listener, make_service_router(proxy_service))
            .await
            .expect("proxy server error");
    });
    server_tasks.0.push(proxy_task);

    let proxy_url = format!("http://127.0.0.1:{proxy_port}/~/graphql/repo.git");
    let result = exercise_graphql(
        &reqwest::Client::new(),
        &proxy_url,
        &service,
        &diagnostic_repo,
        &head,
        &files,
    )
    .await;

    for task in server_tasks.0.drain(..) {
        task.abort();
        let _ = task.await;
    }
    drop(service);
    io_task.await?;
    result
}
