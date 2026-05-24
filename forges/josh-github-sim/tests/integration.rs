use std::process::Command;

use anyhow::{Context, bail};
use josh_cq_test_components::{TestRepo, TreeEntry, TreeMode};
use josh_github_sim::{GithubSim, RepoConfig};

fn git_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("GIT_AUTHOR_NAME", "test"),
        ("GIT_AUTHOR_EMAIL", "test@test.com"),
        ("GIT_COMMITTER_NAME", "test"),
        ("GIT_COMMITTER_EMAIL", "test@test.com"),
    ]
}

fn run_git_sync(
    args: &[&str],
    dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
) -> anyhow::Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    for (k, v) in git_env() {
        cmd.env(k, v);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.output().context("failed to run git")
}

async fn run_git(
    args: Vec<String>,
    dir: Option<std::path::PathBuf>,
    extra_env: Vec<(String, String)>,
) -> anyhow::Result<std::process::Output> {
    tokio::task::spawn_blocking(move || {
        let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let str_env: Vec<(&str, &str)> = extra_env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        run_git_sync(&str_args, dir.as_deref(), &str_env)
    })
    .await
    .context("spawn_blocking panicked")?
}

fn output_to_string(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn check_git_ok(output: &std::process::Output, args: &[String]) -> anyhow::Result<()> {
    if !output.status.success() {
        bail!(
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            output_to_string(output),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    Ok(())
}

#[tokio::test]
async fn query_graphql_get_repository() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;
    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "README.md".into(),
            content: "hello".into(),
        }]),
        "initial",
        "refs/heads/main",
    )
    .await?;

    let sim = GithubSim::new(vec![RepoConfig {
        owner: "acme".into(),
        name: "widgets".into(),
        repo,
    }])
    .await?;

    let client = reqwest::Client::new();
    let query = serde_json::json!({
        "query": "query GetRepository($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { nameWithOwner defaultBranchRef { name } } }",
        "variables": { "owner": "acme", "name": "widgets" },
        "operationName": "GetRepository"
    });

    let response = client
        .post(sim.graphql_url().as_str())
        .json(&query)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await?;
    insta::assert_json_snapshot!(body, @r#"
    {
      "data": {
        "repository": {
          "defaultBranchRef": {
            "name": "main"
          },
          "nameWithOwner": "acme/widgets"
        }
      }
    }
    "#);
    Ok(())
}

#[tokio::test]
async fn unknown_operation_returns_error() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;
    let sim = GithubSim::new(vec![RepoConfig {
        owner: "acme".into(),
        name: "widgets".into(),
        repo,
    }])
    .await?;

    let client = reqwest::Client::new();
    let query = serde_json::json!({
        "query": "query NonExistent { foo }",
        "operationName": "NonExistent"
    });

    let response = client
        .post(sim.graphql_url().as_str())
        .json(&query)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await?;
    insta::assert_json_snapshot!(body, @r#"
    {
      "errors": [
        {
          "locations": [
            {
              "column": 21,
              "line": 1
            }
          ],
          "message": "Unknown field \"foo\" on type \"Query\""
        }
      ]
    }
    "#);
    Ok(())
}

#[tokio::test]
async fn unknown_repo_returns_graphql_error() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;
    let sim = GithubSim::new(vec![RepoConfig {
        owner: "acme".into(),
        name: "widgets".into(),
        repo,
    }])
    .await?;

    let client = reqwest::Client::new();
    let query = serde_json::json!({
        "query": "query GetRepository($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { nameWithOwner } }",
        "variables": { "owner": "acme", "name": "nonexistent" },
        "operationName": "GetRepository"
    });

    let response = client
        .post(sim.graphql_url().as_str())
        .json(&query)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await?;
    insta::assert_json_snapshot!(body, @r#"
    {
      "data": {
        "repository": null
      }
    }
    "#);
    Ok(())
}

#[tokio::test]
async fn git_clone_via_sim() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;
    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "README.md".into(),
            content: "hello world".into(),
        }]),
        "initial",
        "refs/heads/main",
    )
    .await?;

    let sim = GithubSim::new(vec![RepoConfig {
        owner: "acme".into(),
        name: "widgets".into(),
        repo,
    }])
    .await?;

    let clone_dir = tempfile::Builder::new()
        .prefix("josh-sim-clone")
        .tempdir()?;
    let clone_path = clone_dir.path().to_owned();

    let clone_url = format!("{}{}/{}", sim.url(), "acme", "widgets");
    let output = run_git(
        vec!["clone".into(), clone_url, ".".into()],
        Some(clone_path.clone()),
        vec![],
    )
    .await?;
    check_git_ok(&output, &["clone".into()])?;

    let content = std::fs::read_to_string(clone_path.join("README.md"))?;
    assert_eq!(content, "hello world");
    Ok(())
}

#[tokio::test]
async fn multiple_repos() -> anyhow::Result<()> {
    let repo1 = TestRepo::new().await?;
    repo1
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "a.txt".into(),
                content: "alpha".into(),
            }]),
            "initial",
            "refs/heads/main",
        )
        .await?;

    let repo2 = TestRepo::new().await?;
    repo2
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "b.txt".into(),
                content: "beta".into(),
            }]),
            "initial",
            "refs/heads/main",
        )
        .await?;

    let sim = GithubSim::new(vec![
        RepoConfig {
            owner: "org".into(),
            name: "repo1".into(),
            repo: repo1,
        },
        RepoConfig {
            owner: "org".into(),
            name: "repo2".into(),
            repo: repo2,
        },
    ])
    .await?;

    // Clone repo1
    let dir1 = tempfile::Builder::new()
        .prefix("josh-sim-multi1")
        .tempdir()?;
    let path1 = dir1.path().to_owned();
    let url1 = format!("{}{}/{}", sim.url(), "org", "repo1");
    let output = run_git(
        vec!["clone".into(), url1, ".".into()],
        Some(path1.clone()),
        vec![],
    )
    .await?;
    check_git_ok(&output, &["clone".into()])?;
    assert_eq!(std::fs::read_to_string(path1.join("a.txt"))?, "alpha");

    // Clone repo2
    let dir2 = tempfile::Builder::new()
        .prefix("josh-sim-multi2")
        .tempdir()?;
    let path2 = dir2.path().to_owned();
    let url2 = format!("{}{}/{}", sim.url(), "org", "repo2");
    let output = run_git(
        vec!["clone".into(), url2, ".".into()],
        Some(path2.clone()),
        vec![],
    )
    .await?;
    check_git_ok(&output, &["clone".into()])?;
    assert_eq!(std::fs::read_to_string(path2.join("b.txt"))?, "beta");

    // GraphQL query for repo1
    let client = reqwest::Client::new();
    let query = serde_json::json!({
        "query": "query GetRepository($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { nameWithOwner } }",
        "variables": { "owner": "org", "name": "repo1" },
        "operationName": "GetRepository"
    });
    let response = client
        .post(sim.graphql_url().as_str())
        .json(&query)
        .send()
        .await?;
    let body: serde_json::Value = response.json().await?;
    insta::assert_json_snapshot!(body, @r#"
    {
      "data": {
        "repository": {
          "nameWithOwner": "org/repo1"
        }
      }
    }
    "#);

    Ok(())
}
