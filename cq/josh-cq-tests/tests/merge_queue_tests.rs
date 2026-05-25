use josh_cq_test_components::{TestRepo, TreeEntry, TreeMode};
use josh_cq_tests::test_helpers::{init_tracing, start_test_harness};
use josh_github_sim::{GithubSim, MockRuleset, PrStatus, RepoConfig, ReviewState, RuleEnforcement};

#[tokio::test]
async fn merge_single_pr() -> anyhow::Result<()> {
    init_tracing();
    let owner = "test-owner";
    let name = "test-repo";

    // Create TestRepo and set up branches
    let test_repo = TestRepo::new().await?;
    test_repo
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
    test_repo
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

    let harness = start_test_harness(owner, name, github_sim).await?;
    harness
        .github_sim
        .set_webhook_url(url::Url::parse(&harness.cq_webhook_url)?);

    harness.track(owner, name).await?;

    let repo = harness.github_sim.repo_by_name(owner, name);
    let (pr_node_id, number) = repo
        .pr_open("Test PR", "refs/heads/feature", "refs/heads/main")
        .await?;
    repo.add_review(number, "maintainer1", ReviewState::Approved)
        .await?;
    repo.add_maintainer("maintainer1").await?;

    harness.tick().await?;

    assert_eq!(repo.pr_by_node_id(&pr_node_id), Some(PrStatus::Closed));

    let comments = repo.pr_comments_by_node_id(&pr_node_id);
    assert!(
        comments
            .iter()
            .any(|body| body.contains("Merged by Josh merge queue")),
        "Expected merge comment, got: {:?}",
        comments
    );

    Ok(())
}

#[tokio::test]
async fn pr_not_admissible_without_review() -> anyhow::Result<()> {
    let owner = "test-owner";
    let name = "test-repo-norev";

    let test_repo = TestRepo::new().await?;
    test_repo
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
    test_repo
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

    let harness = start_test_harness(owner, name, github_sim).await?;
    harness
        .github_sim
        .set_webhook_url(url::Url::parse(&harness.cq_webhook_url)?);

    harness.track(owner, name).await?;

    let repo = harness.github_sim.repo_by_name(owner, name);
    let (pr_node_id, _number) = repo
        .pr_open("No-review PR", "refs/heads/feature", "refs/heads/main")
        .await?;
    repo.add_maintainer("maintainer1").await?;

    harness.tick().await?;

    assert_eq!(
        repo.pr_by_node_id(&pr_node_id),
        Some(PrStatus::Open),
        "PR should not be merged without an approving review"
    );

    Ok(())
}

#[tokio::test]
async fn pr_not_admissible_with_failing_check() -> anyhow::Result<()> {
    let owner = "test-owner";
    let name = "test-repo-fail";

    let test_repo = TestRepo::new().await?;
    test_repo
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
    test_repo
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

    let harness = start_test_harness(owner, name, github_sim).await?;
    harness
        .github_sim
        .set_webhook_url(url::Url::parse(&harness.cq_webhook_url)?);

    harness.track(owner, name).await?;

    let repo = harness.github_sim.repo_by_name(owner, name);
    let (pr_node_id, number) = repo
        .pr_open("Failing-check PR", "refs/heads/feature", "refs/heads/main")
        .await?;
    repo.add_review(number, "maintainer1", ReviewState::Approved)
        .await?;
    repo.add_maintainer("maintainer1").await?;
    repo.add_ruleset(MockRuleset {
        id: "rs-1".into(),
        name: "test ruleset".into(),
        enforcement: RuleEnforcement::Active,
        include_refs: vec!["refs/heads/main".into()],
        exclude_refs: vec![],
        required_checks: vec!["ci/test".into()],
    })
    .await?;
    repo.complete_check_run("ci/test", number, "failure")
        .await?;

    harness.tick().await?;

    assert_eq!(
        repo.pr_by_node_id(&pr_node_id),
        Some(PrStatus::Open),
        "PR should not be merged with a failing required check"
    );

    Ok(())
}

#[tokio::test]
async fn pr_removed_on_close_webhook() -> anyhow::Result<()> {
    let owner = "test-owner";
    let name = "test-repo-close";

    let test_repo = TestRepo::new().await?;
    test_repo
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
    test_repo
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

    let harness = start_test_harness(owner, name, github_sim).await?;
    harness
        .github_sim
        .set_webhook_url(url::Url::parse(&harness.cq_webhook_url)?);

    harness.track(owner, name).await?;

    let repo = harness.github_sim.repo_by_name(owner, name);
    let (pr_node_id, number) = repo
        .pr_open("Close-test PR", "refs/heads/feature", "refs/heads/main")
        .await?;
    repo.add_review(number, "maintainer1", ReviewState::Approved)
        .await?;
    repo.add_maintainer("maintainer1").await?;

    repo.pr_close(&pr_node_id).await?;

    // Send Tick - PR should NOT be merged because it was closed
    harness.tick().await?;

    assert_eq!(
        repo.pr_by_node_id(&pr_node_id),
        Some(PrStatus::Closed),
        "PR should be closed"
    );
    let comments = repo.pr_comments_by_node_id(&pr_node_id);
    assert!(
        !comments
            .iter()
            .any(|body| body.contains("Merged by Josh merge queue")),
        "PR should not be merged after being closed via webhook, got comments: {:?}",
        comments
    );

    Ok(())
}
