use std::sync::Arc;
use std::sync::Once;
use std::time::Duration;

static INIT_ENV: Once = Once::new();

use josh_cq::types::CqEvent;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .try_init();
}
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;
use josh_test_github::graphql_mock::{GraphQLMock, MockPr, MockRuleset};
use josh_test_github::sim_repo::{SimRepo, make_pr_node_id, make_pr_payload, make_repository};

struct TestHarness {
    event_tx: tokio::sync::mpsc::Sender<CqEvent>,
    sim_repo: Arc<SimRepo>,
    graphql_mock: Arc<GraphQLMock>,
    #[allow(dead_code)]
    _metarepo_temp: tempfile::TempDir,
    #[allow(dead_code)]
    _cache: Arc<josh_core::cache::CacheStack>,
    #[allow(dead_code)]
    _graphql_handle: tokio::task::JoinHandle<()>,
}

async fn start_test_harness(
    _owner: &str,
    _name: &str,
    sim_repo: Arc<SimRepo>,
    mock: GraphQLMock,
) -> anyhow::Result<TestHarness> {
    INIT_ENV.call_once(|| {
        unsafe { std::env::set_var("JOSH_EXPERIMENTAL_FEATURES", "1") };
    });

    // 1. Create metarepo with an initial commit so HEAD exists
    let metarepo_temp = tempfile::Builder::new().prefix("josh-cq-test-").tempdir()?;
    let metarepo_path = metarepo_temp.path();

    let repo = git2::Repository::init(metarepo_path)?;
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
    let git_dir = repo.path().to_path_buf();
    let repo_path = josh_core::git::normalize_repo_path(&git_dir);
    drop(repo);

    // 2. Initialize cache
    josh_core::cache::sled_load(&repo_path.join(".git"))?;
    let cache: Arc<josh_core::cache::CacheStack> =
        Arc::new(josh_core::cache::CacheStack::default());
    let ctx = josh_core::cache::TransactionContext::new(&repo_path, cache.clone());
    let transaction = ctx.open(None)?;

    // 3. handle_init
    josh_cq::init::handle_init(&transaction)?;

    // 4. Start GraphQL mock server
    let graphql_mock = Arc::new(mock);
    let (graphql_handle, graphql_url) = graphql_mock.serve().await?;

    // 5. Create mock API connection
    let api = Arc::new(GithubApiConnection::for_test(graphql_url));

    // 6. Track the SimRepo in the metarepo
    josh_cq::track::handle_track(
        sim_repo.clone_url().as_str(),
        "test-remote",
        "snapshot",
        &transaction,
    )?;
    drop(transaction);

    // 7. Build URL → owner/name mapping so the CQ actor can resolve
    // non-GitHub URLs (e.g. 127.0.0.1) from the SimRepo's clone URL.
    let mut url_owner_map = std::collections::HashMap::new();
    url_owner_map.insert(
        sim_repo.clone_url().to_string(),
        (_owner.to_string(), _name.to_string()),
    );

    // 8. Start the CQ actor (long tick interval so we drive ticks manually)
    let event_tx =
        josh_cq::server::spawn_serve_task(repo_path, cache.clone(), 3600, Some(api), url_owner_map);

    Ok(TestHarness {
        event_tx,
        sim_repo,
        graphql_mock,
        _metarepo_temp: metarepo_temp,
        _cache: cache,
        _graphql_handle: graphql_handle,
    })
}

async fn poll_until(
    mut condition: impl FnMut() -> bool,
    timeout: Duration,
    interval: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(interval).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn merge_single_pr() {
    init_tracing();
    let owner = "test-owner";
    let name = "test-repo";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    // Create SimRepo and set up branches
    let sim_repo = Arc::new(SimRepo::new(owner, name, None).await.unwrap());
    let (main_sha, _) = sim_repo
        .commit("README.md", "# test", Some("initial"))
        .await
        .unwrap();
    sim_repo.select_create_branch("feature").await.unwrap();
    let (feature_sha, _) = sim_repo
        .commit("feature.txt", "feature content", Some("feature wip"))
        .await
        .unwrap();

    let mock = GraphQLMock::new()
        .with_pr(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "Test PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        })
        .with_review(0, "maintainer1", "APPROVED")
        .with_maintainer("maintainer1");

    let harness = start_test_harness(owner, name, sim_repo.clone(), mock)
        .await
        .unwrap();

    let clone_url = harness.sim_repo.clone_url().to_string();

    harness.sim_repo.open_pr("feature", "main").await.unwrap();

    // Send PullRequest Opened webhook
    let payload = WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
        pull_request: make_pr_payload(
            owner,
            name,
            0,
            "refs/heads/feature",
            &feature_sha.to_string(),
            "refs/heads/main",
            &main_sha.to_string(),
        ),
        repository: make_repository(&clone_url),
        details: webhook_types::PullRequestEventDetails::Opened,
    }));
    harness
        .event_tx
        .send(CqEvent::Webhook(payload))
        .await
        .unwrap();

    harness.event_tx.send(CqEvent::Tick).await.unwrap();
    let merged = poll_until(
        || !harness.graphql_mock.closed_pr_node_ids().is_empty(),
        Duration::from_secs(30),
        Duration::from_millis(100),
    )
    .await;

    assert!(merged, "PR should have been merged within 30 seconds");
    assert!(
        harness
            .graphql_mock
            .closed_pr_node_ids()
            .contains(&pr_node_id)
    );

    let comments = harness.graphql_mock.comments();
    assert!(
        comments
            .iter()
            .any(|(subj, body)| subj == &pr_node_id && body.contains("Merged by Josh merge queue")),
        "Expected merge comment, got: {:?}",
        comments
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn pr_not_admissible_without_review() {
    let owner = "test-owner";
    let name = "test-repo-norev";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    let sim_repo = Arc::new(SimRepo::new(owner, name, None).await.unwrap());
    let (main_sha, _) = sim_repo
        .commit("README.md", "# test", Some("initial"))
        .await
        .unwrap();
    sim_repo.select_create_branch("feature").await.unwrap();
    let (feature_sha, _) = sim_repo
        .commit("feature.txt", "content", Some("feature"))
        .await
        .unwrap();

    let mock = GraphQLMock::new()
        .with_pr(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "No-review PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        })
        .with_maintainer("maintainer1");

    let harness = start_test_harness(owner, name, sim_repo.clone(), mock)
        .await
        .unwrap();

    let clone_url = harness.sim_repo.clone_url().to_string();

    harness.sim_repo.open_pr("feature", "main").await.unwrap();

    let payload = WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
        pull_request: make_pr_payload(
            owner,
            name,
            0,
            "refs/heads/feature",
            &feature_sha.to_string(),
            "refs/heads/main",
            &main_sha.to_string(),
        ),
        repository: make_repository(&clone_url),
        details: webhook_types::PullRequestEventDetails::Opened,
    }));
    harness
        .event_tx
        .send(CqEvent::Webhook(payload))
        .await
        .unwrap();

    harness.event_tx.send(CqEvent::Tick).await.unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        harness.graphql_mock.closed_pr_node_ids().is_empty(),
        "PR should not be merged without an approving review"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn pr_not_admissible_with_failing_check() {
    let owner = "test-owner";
    let name = "test-repo-fail";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    let sim_repo = Arc::new(SimRepo::new(owner, name, None).await.unwrap());
    let (main_sha, _) = sim_repo
        .commit("README.md", "# test", Some("initial"))
        .await
        .unwrap();
    sim_repo.select_create_branch("feature").await.unwrap();
    let (feature_sha, _) = sim_repo
        .commit("feature.txt", "content", Some("feature"))
        .await
        .unwrap();

    let mock = GraphQLMock::new()
        .with_pr(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "Failing-check PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        })
        .with_review(0, "maintainer1", "APPROVED")
        .with_maintainer("maintainer1")
        .with_ruleset(MockRuleset {
            id: "rs-1".into(),
            name: "test ruleset".into(),
            enforcement: "ACTIVE".into(),
            include_refs: vec!["refs/heads/main".into()],
            exclude_refs: vec![],
        })
        .with_required_check("ci/test");

    let harness = start_test_harness(owner, name, sim_repo.clone(), mock)
        .await
        .unwrap();

    let clone_url = harness.sim_repo.clone_url().to_string();

    harness.sim_repo.open_pr("feature", "main").await.unwrap();

    // Send PR opened webhook
    let payload = WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
        pull_request: make_pr_payload(
            owner,
            name,
            0,
            "refs/heads/feature",
            &feature_sha.to_string(),
            "refs/heads/main",
            &main_sha.to_string(),
        ),
        repository: make_repository(&clone_url),
        details: webhook_types::PullRequestEventDetails::Opened,
    }));
    harness
        .event_tx
        .send(CqEvent::Webhook(payload))
        .await
        .unwrap();

    // Send failing check run webhook
    let check_payload = WebhookPayload::CheckRun(Box::new(webhook_types::CheckRunEvent {
        check_run: webhook_types::CheckRun {
            id: 1,
            name: "ci/test".to_string(),
            head_sha: feature_sha.to_string(),
            status: "completed".to_string(),
            conclusion: Some(webhook_types::CheckRunConclusion::Failure),
            started_at: Default::default(),
            completed_at: None,
        },
        repository: make_repository(&clone_url),
        details: webhook_types::CheckRunEventDetails::Completed,
    }));
    harness
        .event_tx
        .send(CqEvent::Webhook(check_payload))
        .await
        .unwrap();

    harness.event_tx.send(CqEvent::Tick).await.unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        harness.graphql_mock.closed_pr_node_ids().is_empty(),
        "PR should not be merged with a failing required check"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn pr_removed_on_close_webhook() {
    let owner = "test-owner";
    let name = "test-repo-close";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    let sim_repo = Arc::new(SimRepo::new(owner, name, None).await.unwrap());
    let (main_sha, _) = sim_repo
        .commit("README.md", "# test", Some("initial"))
        .await
        .unwrap();
    sim_repo.select_create_branch("feature").await.unwrap();
    let (feature_sha, _) = sim_repo
        .commit("feature.txt", "content", Some("feature"))
        .await
        .unwrap();

    let mock = GraphQLMock::new()
        .with_pr(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "Close-test PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        })
        .with_review(0, "maintainer1", "APPROVED")
        .with_maintainer("maintainer1");

    let harness = start_test_harness(owner, name, sim_repo.clone(), mock)
        .await
        .unwrap();

    let clone_url = harness.sim_repo.clone_url().to_string();

    harness.sim_repo.open_pr("feature", "main").await.unwrap();

    // Send PR opened webhook
    let payload = WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
        pull_request: make_pr_payload(
            owner,
            name,
            0,
            "refs/heads/feature",
            &feature_sha.to_string(),
            "refs/heads/main",
            &main_sha.to_string(),
        ),
        repository: make_repository(&clone_url),
        details: webhook_types::PullRequestEventDetails::Opened,
    }));
    harness
        .event_tx
        .send(CqEvent::Webhook(payload))
        .await
        .unwrap();

    // Send PR closed webhook
    let closed_payload = WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
        pull_request: make_pr_payload(
            owner,
            name,
            0,
            "refs/heads/feature",
            &feature_sha.to_string(),
            "refs/heads/main",
            &main_sha.to_string(),
        ),
        repository: make_repository(&clone_url),
        details: webhook_types::PullRequestEventDetails::Closed,
    }));
    harness
        .event_tx
        .send(CqEvent::Webhook(closed_payload))
        .await
        .unwrap();

    // Send Tick — PR should NOT be merged because it was closed
    harness.event_tx.send(CqEvent::Tick).await.unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        harness.graphql_mock.closed_pr_node_ids().is_empty(),
        "PR should not be merged after being closed via webhook"
    );
}
