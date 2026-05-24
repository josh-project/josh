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

use josh_cq_test_components::{TestRepo, TreeEntry, TreeMode};
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_sim::{GithubSim, MockPr, MockRuleset, RepoConfig};
use josh_github_webhooks::test_helpers::{make_pr_node_id, make_pr_payload, make_repository};
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;

struct TestHarness {
    event_tx: tokio::sync::mpsc::Sender<CqEvent>,
    github_sim: GithubSim,
    #[allow(dead_code)]
    _metarepo_temp: tempfile::TempDir,
    #[allow(dead_code)]
    _cache: Arc<josh_core::cache::CacheStack>,
}

async fn start_test_harness(
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

    // 4. GraphQL URL — GithubSim serves GraphQL at /graphql
    let api = Arc::new(GithubApiConnection::for_test(
        github_sim.graphql_url().clone(),
    ));

    // 5. Track the repo in the metarepo
    //    GithubSim uses /owner/name path prefix for git HTTP routing
    let git_url = format!("{}{}/{}", github_sim.url(), owner, name);
    let track_url = url::Url::parse(&git_url)?;
    tokio::task::spawn_blocking({
        let track_url = track_url.clone();
        move || {
            josh_cq::track::handle_track(
                track_url.as_str(),
                "test-remote",
                "snapshot",
                &transaction,
            )
        }
    })
    .await??;

    // 6. Build URL → owner/name mapping so the CQ actor can resolve
    //    non-GitHub URLs from the GithubSim's git URL.
    let mut url_owner_map = std::collections::HashMap::new();
    url_owner_map.insert(track_url.to_string(), (owner.to_string(), name.to_string()));

    // 7. Start the CQ actor (long tick interval so we drive ticks manually)
    let event_tx =
        josh_cq::server::spawn_serve_task(repo_path, cache.clone(), 3600, Some(api), url_owner_map);

    Ok(TestHarness {
        event_tx,
        github_sim,
        _metarepo_temp: metarepo_temp,
        _cache: cache,
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

#[tokio::test]
async fn merge_single_pr() -> anyhow::Result<()> {
    init_tracing();
    let owner = "test-owner";
    let name = "test-repo";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    // Create TestRepo and set up branches
    let test_repo = TestRepo::new().await?;
    let main_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "README.md".into(),
                content: "# test".into(),
            }]),
            "initial",
            "refs/heads/main",
        )
        .await?;
    test_repo
        .create_branch("feature", "refs/heads/main")
        .await?;
    let feature_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "feature.txt".into(),
                content: "feature content".into(),
            }]),
            "feature wip",
            "refs/heads/feature",
        )
        .await?;

    let github_sim = GithubSim::new(vec![RepoConfig {
        owner: owner.to_string(),
        name: name.to_string(),
        repo: test_repo,
    }])
    .await?;

    {
        let mut state = github_sim.graphql_state().lock().unwrap();
        state.prs.push(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "Test PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        });
        state
            .reviews
            .insert(0, vec![("maintainer1".to_string(), "APPROVED".to_string())]);
        state.maintainers.push("maintainer1".to_string());
    }

    let harness = start_test_harness(owner, name, github_sim).await?;

    let clone_url = format!("{}{}/{}", harness.github_sim.url(), owner, name);

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
    harness.event_tx.send(CqEvent::Webhook(payload)).await?;

    harness.event_tx.send(CqEvent::Tick).await?;
    let merged = poll_until(
        || !harness.github_sim.closed_pr_node_ids().is_empty(),
        Duration::from_secs(30),
        Duration::from_millis(100),
    )
    .await;

    assert!(merged, "PR should have been merged within 30 seconds");
    assert!(
        harness
            .github_sim
            .closed_pr_node_ids()
            .contains(&pr_node_id)
    );

    let comments = harness.github_sim.comments();
    assert!(
        comments
            .iter()
            .any(|(subj, body)| subj == &pr_node_id && body.contains("Merged by Josh merge queue")),
        "Expected merge comment, got: {:?}",
        comments
    );

    Ok(())
}

#[tokio::test]
async fn pr_not_admissible_without_review() -> anyhow::Result<()> {
    let owner = "test-owner";
    let name = "test-repo-norev";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    let test_repo = TestRepo::new().await?;
    let main_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "README.md".into(),
                content: "# test".into(),
            }]),
            "initial",
            "refs/heads/main",
        )
        .await?;
    test_repo
        .create_branch("feature", "refs/heads/main")
        .await?;
    let feature_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "feature.txt".into(),
                content: "content".into(),
            }]),
            "feature",
            "refs/heads/feature",
        )
        .await?;

    let github_sim = GithubSim::new(vec![RepoConfig {
        owner: owner.to_string(),
        name: name.to_string(),
        repo: test_repo,
    }])
    .await?;

    {
        let mut state = github_sim.graphql_state().lock().unwrap();
        state.prs.push(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "No-review PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        });
        state.maintainers.push("maintainer1".to_string());
    }

    let harness = start_test_harness(owner, name, github_sim).await?;

    let clone_url = format!("{}{}/{}", harness.github_sim.url(), owner, name);

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
    harness.event_tx.send(CqEvent::Webhook(payload)).await?;

    harness.event_tx.send(CqEvent::Tick).await?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        harness.github_sim.closed_pr_node_ids().is_empty(),
        "PR should not be merged without an approving review"
    );

    Ok(())
}

#[tokio::test]
async fn pr_not_admissible_with_failing_check() -> anyhow::Result<()> {
    let owner = "test-owner";
    let name = "test-repo-fail";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    let test_repo = TestRepo::new().await?;
    let main_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "README.md".into(),
                content: "# test".into(),
            }]),
            "initial",
            "refs/heads/main",
        )
        .await?;
    test_repo
        .create_branch("feature", "refs/heads/main")
        .await?;
    let feature_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "feature.txt".into(),
                content: "content".into(),
            }]),
            "feature",
            "refs/heads/feature",
        )
        .await?;

    let github_sim = GithubSim::new(vec![RepoConfig {
        owner: owner.to_string(),
        name: name.to_string(),
        repo: test_repo,
    }])
    .await?;

    {
        let mut state = github_sim.graphql_state().lock().unwrap();
        state.prs.push(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "Failing-check PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        });
        state
            .reviews
            .insert(0, vec![("maintainer1".to_string(), "APPROVED".to_string())]);
        state.maintainers.push("maintainer1".to_string());
        state.rulesets.push(MockRuleset {
            id: "rs-1".into(),
            name: "test ruleset".into(),
            enforcement: "ACTIVE".into(),
            include_refs: vec!["refs/heads/main".into()],
            exclude_refs: vec![],
            required_checks: vec!["ci/test".into()],
        });
    }

    let harness = start_test_harness(owner, name, github_sim).await?;

    let clone_url = format!("{}{}/{}", harness.github_sim.url(), owner, name);

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
    harness.event_tx.send(CqEvent::Webhook(payload)).await?;

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
        .await?;

    harness.event_tx.send(CqEvent::Tick).await?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        harness.github_sim.closed_pr_node_ids().is_empty(),
        "PR should not be merged with a failing required check"
    );

    Ok(())
}

#[tokio::test]
async fn pr_removed_on_close_webhook() -> anyhow::Result<()> {
    let owner = "test-owner";
    let name = "test-repo-close";
    let pr_node_id = make_pr_node_id(owner, name, 0);

    let test_repo = TestRepo::new().await?;
    let main_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "README.md".into(),
                content: "# test".into(),
            }]),
            "initial",
            "refs/heads/main",
        )
        .await?;
    test_repo
        .create_branch("feature", "refs/heads/main")
        .await?;
    let feature_sha = test_repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "feature.txt".into(),
                content: "content".into(),
            }]),
            "feature",
            "refs/heads/feature",
        )
        .await?;

    let github_sim = GithubSim::new(vec![RepoConfig {
        owner: owner.to_string(),
        name: name.to_string(),
        repo: test_repo,
    }])
    .await?;

    {
        let mut state = github_sim.graphql_state().lock().unwrap();
        state.prs.push(MockPr {
            node_id: pr_node_id.clone(),
            number: 0,
            title: "Close-test PR".into(),
            head_ref_name: "feature".into(),
            head_ref_oid: feature_sha.to_string(),
            base_ref_name: "main".into(),
            base_ref_oid: main_sha.to_string(),
        });
        state
            .reviews
            .insert(0, vec![("maintainer1".to_string(), "APPROVED".to_string())]);
        state.maintainers.push("maintainer1".to_string());
    }

    let harness = start_test_harness(owner, name, github_sim).await?;

    let clone_url = format!("{}{}/{}", harness.github_sim.url(), owner, name);

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
    harness.event_tx.send(CqEvent::Webhook(payload)).await?;

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
        .await?;

    // Send Tick - PR should NOT be merged because it was closed
    harness.event_tx.send(CqEvent::Tick).await?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        harness.github_sim.closed_pr_node_ids().is_empty(),
        "PR should not be merged after being closed via webhook"
    );

    Ok(())
}
