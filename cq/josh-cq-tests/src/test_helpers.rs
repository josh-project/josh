use std::sync::Arc;
use std::sync::Once;

use josh_command_middleware::CommandStack;
use josh_cq::types::CqEvent;
use josh_github_auth::middleware::GithubAuthMiddleware;
use josh_github_sim::GithubSim;

static INIT_ENV: Once = Once::new();

pub fn init_tracing() {
    let directives = [
        "info",
        "josh_core::history=warn",
        "josh_core::filter=warn",
        "josh_core::cache::history_graph=warn",
    ];

    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(directives.join(",")))
        .try_init();
}

pub struct TestHarness {
    pub event_tx: tokio::sync::mpsc::Sender<CqEvent>,
    pub github_sim: GithubSim,
    pub cq_webhook_url: String,
    #[allow(dead_code)]
    _cq_server: tokio::task::JoinHandle<()>,
    #[allow(dead_code)]
    _metarepo_temp: tempfile::TempDir,
    #[allow(dead_code)]
    _cache: Arc<josh_core::cache::CacheStack>,
}

impl TestHarness {
    pub async fn tick(&self) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.event_tx.send(CqEvent::Tick { done: Some(tx) }).await?;
        rx.await??;
        Ok(())
    }

    pub async fn track(&self, owner: &str, name: &str) -> anyhow::Result<()> {
        let url = format!("{}{}/{}", self.github_sim.url(), owner, name);
        let body = serde_json::json!({
            "url": url,
            "id": "test-remote",
        });
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/track", self.cq_webhook_url))
            .json(&body)
            .send()
            .await?;
        anyhow::ensure!(
            resp.status().is_success(),
            "track request failed: {}",
            resp.status()
        );
        Ok(())
    }
}

pub async fn start_test_harness(
    owner: &str,
    name: &str,
    github_sim: GithubSim,
) -> anyhow::Result<TestHarness> {
    INIT_ENV.call_once(|| {
        unsafe { std::env::set_var("JOSH_EXPERIMENTAL_FEATURES", "1") };
    });

    // 1. Create metarepo with an initial commit so HEAD exists
    let metarepo_temp = tempfile::Builder::new().prefix("josh-cq-test-").tempdir()?;
    let metarepo_path = metarepo_temp.path();

    let repo = git2::Repository::init_bare(metarepo_path)?;
    {
        let sig = git2::Signature::new("test", "test@test.com", &git2::Time::new(0, 0))?;
        let tree_oid = repo.treebuilder(None)?.write()?;
        let tree = repo.find_tree(tree_oid)?;
        repo.commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            "initial metarepo",
            &tree,
            &[],
        )?;
    }
    let repo_path = repo.path().to_path_buf();
    drop(repo);

    // 2. Initialize cache
    josh_core::cache::sled_load(&repo_path)?;
    let cache: Arc<josh_core::cache::CacheStack> =
        Arc::new(josh_core::cache::CacheStack::default());
    let ctx = josh_core::cache::TransactionContext::new(&repo_path, cache.clone());
    let transaction = ctx.open(None)?;

    // 3. handle_init
    josh_cq::init::handle_init(&transaction)?;

    // 4. Build URL → owner/name mapping so the CQ actor can resolve
    //    non-GitHub URLs from the GithubSim's git URL.
    let git_url = format!("{}{}/{}", github_sim.url(), owner, name);
    let mut url_owner_map = std::collections::HashMap::new();
    url_owner_map.insert(git_url, (owner.to_string(), name.to_string()));

    // 5. Start the CQ actor (long tick interval so we drive ticks manually).
    //    The sim ignores auth, so a dummy-token middleware suffices; the
    //    GraphQL connection is pointed at GithubSim's /graphql endpoint.
    let middleware = Arc::new(GithubAuthMiddleware::from_token("test-token"));
    let command_env = CommandStack::new().layer(middleware.clone());
    let git = josh_cq::git::spawn_git_actor(repo_path, cache.clone(), command_env);
    let event_tx = josh_cq::server::spawn_serve_task(
        3600,
        git,
        middleware,
        Some(github_sim.graphql_url().clone()),
        url_owner_map,
    );

    // 7. Start the CQ HTTP server so webhooks go through the real HTTP path
    let (cq_server, cq_webhook_url) = josh_cq::server::bind_router(event_tx.clone()).await?;

    Ok(TestHarness {
        event_tx,
        github_sim,
        cq_webhook_url,
        _cq_server: cq_server,
        _metarepo_temp: metarepo_temp,
        _cache: cache,
    })
}
