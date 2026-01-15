use josh_core::cache::CacheStack;
use josh_proxy::service::{JoshProxyService, JoshProxyUpstream, make_service_router};

use axum::response::IntoResponse;
use axum::{Router, extract::Request, response::Response, routing::any};
use clap::Parser;
use reqwest::StatusCode;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    upstream_dir: std::path::PathBuf,

    #[arg(long)]
    proxy_dir: std::path::PathBuf,

    #[arg(long)]
    local_dir: std::path::PathBuf,

    /// Source ref to use for benchmark
    #[arg(long, default_value = "refs/heads/master")]
    source_ref: String,

    /// Ignore --bench flag passed by cargo bench
    #[arg(long)]
    bench: bool,

    /// Path to josh-proxy binary for git hooks
    #[arg(long)]
    josh_proxy_path: std::path::PathBuf,
}

async fn git_handler(upstream_dir: std::path::PathBuf, req: Request) -> Response {
    let path = req.uri().path().to_string();

    // Strip /repo.git prefix if present, since GIT_PROJECT_ROOT points to the repo itself
    let git_path = path.strip_prefix("/repo.git").unwrap_or(&path);

    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("http-backend");
    cmd.current_dir(&upstream_dir);
    cmd.env("GIT_PROJECT_ROOT", &upstream_dir);
    cmd.env("GIT_HTTP_EXPORT_ALL", "1");
    cmd.env("PATH_INFO", git_path);

    let (response, stream) = match axum_cgi::do_cgi(req, cmd).await {
        Ok((r, s)) => (r, s),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    response
        .body(axum::body::Body::from_stream(stream))
        .expect("failed to build response")
        .into_response()
}

async fn start_git_server(upstream_dir: &std::path::Path) -> tokio::task::JoinHandle<()> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(upstream_dir)
        .args(["config", "http.receivepack", "true"])
        .status()
        .expect("Failed to configure upstream repo");

    let upstream_dir = upstream_dir.to_path_buf();
    let git_app = Router::new().route(
        "/{*path}",
        any(move |req| git_handler(upstream_dir.clone(), req)),
    );

    let git_listener = tokio::net::TcpListener::bind("127.0.0.1:8001")
        .await
        .expect("Failed to bind git server");

    tokio::spawn(async move {
        axum::serve(git_listener, git_app)
            .await
            .expect("Git server error");
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let repo_path = args.proxy_dir;

    josh_proxy::service::create_repo(&repo_path, Some(&args.josh_proxy_path))
        .expect("Failed to create repo");
    josh_core::cache::sled_load(&repo_path).expect("Failed to load cache");

    let git_server_task = start_git_server(&args.upstream_dir).await;

    let proxy_task = {
        let cache = Arc::new(CacheStack::default());
        let proxy_service = Arc::new(JoshProxyService {
            port: "8000".to_string(),
            repo_path: repo_path.clone(),
            upstream: JoshProxyUpstream::Http("http://127.0.0.1:8001".to_string()),
            require_auth: false,
            poll_user: None,
            cache_duration: 0,
            filter_prefix: None,
            cache,
            fetch_timers: Default::default(),
            head_symref_map: Default::default(),
            poll: Default::default(),
            fetch_permits: Default::default(),
            filter_permits: Arc::new(tokio::sync::Semaphore::new(10)),
        });

        let app = make_service_router(proxy_service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
            .await
            .expect("Failed to bind");

        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("Server error");
        })
    };

    let source_repo = git2::Repository::open(&args.local_dir)?;

    let source_ref = source_repo.find_reference(&args.source_ref)?;
    let current_commit = source_ref.peel_to_commit()?;
    let tree = current_commit.tree()?;

    let sig = git2::Signature::now("Benchmark", "benchmark@example.com")?;

    let new_commit_oid = source_repo.commit(
        None,
        &sig,
        &sig,
        "Benchmark commit",
        &tree,
        &[&current_commit],
    )?;

    source_repo.reference("refs/heads/benchmark", new_commit_oid, true, "benchmark")?;

    println!("Created benchmark commit: {}", new_commit_oid);
    let branch = args
        .source_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(args.source_ref.as_str());

    let start = std::time::Instant::now();
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&args.local_dir)
        .arg("push")
        .arg("--force")
        .arg("http://127.0.0.1:8000/repo.git")
        .arg("refs/heads/benchmark")
        .args(["-o", &format!("base={}", branch)])
        .args(["-o", "force"])
        .output()
        .await?;

    let duration = start.elapsed();

    if !output.status.success() {
        println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
    } else {
        println!("Push completed in {:?}", duration);
    }

    proxy_task.abort();
    git_server_task.abort();

    Ok(())
}
