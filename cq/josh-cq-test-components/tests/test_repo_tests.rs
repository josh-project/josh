use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, bail};
use josh_cq_test_components::{TestRepo, TreeEntry, TreeMode};

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

fn git_dir_env(repo_path: &std::path::Path) -> anyhow::Result<(String, String)> {
    Ok((
        "GIT_DIR".to_string(),
        repo_path
            .to_str()
            .context("repo path is not valid UTF-8")?
            .to_string(),
    ))
}

#[tokio::test]
async fn create_empty_repo() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;
    assert!(repo.path().exists());
    assert!(repo.url().port().is_some());

    let output = run_git(
        vec!["ls-remote".into(), repo.url().to_string()],
        None,
        vec![],
    )
    .await?;
    assert!(output.status.success());
    Ok(())
}

#[tokio::test]
async fn commit_first_commit() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;

    let oid = repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "README.md".into(),
                content: "hello".into(),
            }]),
            "initial commit",
            "refs/heads/main",
        )
        .await?;

    let head = repo.get_head("refs/heads/main").await?;
    assert_eq!(oid, head);

    let oid_str = oid.to_string();
    let output = run_git(
        vec!["ls-tree".into(), "-r".into(), oid_str],
        None,
        vec![git_dir_env(repo.path())?],
    )
    .await?;
    check_git_ok(&output, &["ls-tree".into()])?;
    let stdout = output_to_string(&output);
    assert!(stdout.contains("README.md"), "tree output: {}", stdout);
    Ok(())
}

#[tokio::test]
async fn commit_overlay() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;

    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "a.txt".into(),
            content: "A".into(),
        }]),
        "add a",
        "refs/heads/main",
    )
    .await?;

    repo.commit(
        TreeMode::Overlay(vec![TreeEntry {
            path: "b.txt".into(),
            content: "B".into(),
        }]),
        "add b",
        "refs/heads/main",
    )
    .await?;

    let head = repo.get_head("refs/heads/main").await?;
    let output = run_git(
        vec!["ls-tree".into(), "-r".into(), head.to_string()],
        None,
        vec![git_dir_env(repo.path())?],
    )
    .await?;
    check_git_ok(&output, &["ls-tree".into()])?;
    let stdout = output_to_string(&output);
    assert!(stdout.contains("a.txt"), "should have a.txt: {}", stdout);
    assert!(stdout.contains("b.txt"), "should have b.txt: {}", stdout);
    Ok(())
}

#[tokio::test]
async fn commit_replace() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;

    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "a.txt".into(),
            content: "A".into(),
        }]),
        "add a",
        "refs/heads/main",
    )
    .await?;

    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "b.txt".into(),
            content: "B".into(),
        }]),
        "replace with b only",
        "refs/heads/main",
    )
    .await?;

    let head = repo.get_head("refs/heads/main").await?;
    let output = run_git(
        vec!["ls-tree".into(), "-r".into(), head.to_string()],
        None,
        vec![git_dir_env(repo.path())?],
    )
    .await?;
    check_git_ok(&output, &["ls-tree".into()])?;
    let stdout = output_to_string(&output);
    assert!(
        !stdout.contains("a.txt"),
        "should NOT have a.txt: {}",
        stdout
    );
    assert!(stdout.contains("b.txt"), "should have b.txt: {}", stdout);
    Ok(())
}

#[tokio::test]
async fn create_branch() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;

    let main_oid = repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "file.txt".into(),
                content: "content".into(),
            }]),
            "commit on main",
            "refs/heads/main",
        )
        .await?;

    let branch_oid = repo.create_branch("feature", "refs/heads/main").await?;

    assert_eq!(main_oid, branch_oid);
    let head = repo.get_head("refs/heads/feature").await?;
    assert_eq!(main_oid, head);
    Ok(())
}

#[tokio::test]
async fn create_branch_from_oid() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;

    let first_oid = repo
        .commit(
            TreeMode::Replace(vec![TreeEntry {
                path: "file.txt".into(),
                content: "v1".into(),
            }]),
            "first",
            "refs/heads/main",
        )
        .await?;

    repo.commit(
        TreeMode::Overlay(vec![TreeEntry {
            path: "file.txt".into(),
            content: "v2".into(),
        }]),
        "second",
        "refs/heads/main",
    )
    .await?;

    let branch_oid = repo
        .create_branch("old-main", &first_oid.to_string())
        .await?;

    assert_eq!(first_oid, branch_oid);
    let head = repo.get_head("refs/heads/old-main").await?;
    assert_eq!(first_oid, head);
    Ok(())
}

#[tokio::test]
async fn git_clone_via_http() -> anyhow::Result<()> {
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

    let clone_dir = tempfile::Builder::new()
        .prefix("josh-test-clone")
        .tempdir()?;
    let clone_path = clone_dir.path().to_owned();
    let url = repo.url().to_string();

    let output = run_git(
        vec!["clone".into(), url, ".".into()],
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
async fn git_push_via_http() -> anyhow::Result<()> {
    let repo = TestRepo::new().await?;

    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "a.txt".into(),
            content: "initial".into(),
        }]),
        "initial",
        "refs/heads/main",
    )
    .await?;

    let clone_dir = tempfile::Builder::new()
        .prefix("josh-test-push")
        .tempdir()?;
    let clone_path = clone_dir.path().to_owned();
    let url = repo.url().to_string();

    // Clone
    run_git(
        vec!["clone".into(), url.clone(), ".".into()],
        Some(clone_path.clone()),
        vec![],
    )
    .await?;

    // Make change and push
    std::fs::write(clone_path.join("b.txt"), "pushed")?;
    run_git(
        vec!["add".into(), "b.txt".into()],
        Some(clone_path.clone()),
        vec![],
    )
    .await?;
    run_git(
        vec!["commit".into(), "-m".into(), "push commit".into()],
        Some(clone_path.clone()),
        vec![],
    )
    .await?;
    run_git(
        vec!["push".into(), "origin".into(), "main".into()],
        Some(clone_path.clone()),
        vec![],
    )
    .await?;

    let head = repo.get_head("refs/heads/main").await?;
    let output = run_git(
        vec!["ls-tree".into(), "-r".into(), head.to_string()],
        None,
        vec![git_dir_env(repo.path())?],
    )
    .await?;
    check_git_ok(&output, &["ls-tree".into()])?;
    let stdout = output_to_string(&output);
    assert!(stdout.contains("a.txt"), "should have a.txt: {}", stdout);
    assert!(stdout.contains("b.txt"), "should have b.txt: {}", stdout);
    Ok(())
}

#[tokio::test]
async fn concurrent_api_and_http() -> anyhow::Result<()> {
    let repo = Arc::new(TestRepo::new().await?);

    repo.commit(
        TreeMode::Replace(vec![TreeEntry {
            path: "base.txt".into(),
            content: "base".into(),
        }]),
        "base",
        "refs/heads/main",
    )
    .await?;

    let repo_api = repo.clone();
    let task_a = tokio::spawn(async move {
        for i in 0..5 {
            repo_api
                .commit(
                    TreeMode::Overlay(vec![TreeEntry {
                        path: format!("api_{}.txt", i),
                        content: format!("api {}", i),
                    }]),
                    &format!("api commit {}", i),
                    "refs/heads/main",
                )
                .await
                .unwrap();
        }
    });

    let repo_http = repo.clone();
    let task_b = tokio::spawn(async move {
        for i in 0..5 {
            let clone_dir = tempfile::Builder::new()
                .prefix("josh-test-concurrent")
                .tempdir()
                .unwrap();
            let clone_path = clone_dir.path().to_owned();
            let url = repo_http.url().to_string();

            // Clone
            run_git(
                vec!["clone".into(), url.clone(), ".".into()],
                Some(clone_path.clone()),
                vec![],
            )
            .await
            .unwrap();

            // Make change and push
            std::fs::write(
                clone_path.join(format!("http_{}.txt", i)),
                format!("http {}", i),
            )
            .unwrap();
            run_git(
                vec!["add".into(), ".".into()],
                Some(clone_path.clone()),
                vec![],
            )
            .await
            .unwrap();
            run_git(
                vec!["commit".into(), "-m".into(), format!("http commit {}", i)],
                Some(clone_path.clone()),
                vec![],
            )
            .await
            .unwrap();
            run_git(
                vec!["push".into(), "origin".into(), "main".into()],
                Some(clone_path),
                vec![],
            )
            .await
            .unwrap();
        }
    });

    let (a_result, b_result) = tokio::join!(task_a, task_b);
    a_result?;
    b_result?;

    let head = repo.get_head("refs/heads/main").await?;
    let output = run_git(
        vec!["ls-tree".into(), "-r".into(), head.to_string()],
        None,
        vec![git_dir_env(repo.path())?],
    )
    .await?;
    check_git_ok(&output, &["ls-tree".into()])?;
    let stdout = output_to_string(&output);

    assert!(stdout.contains("base.txt"), "missing base.txt: {}", stdout);
    for i in 0..5 {
        let api_name = format!("api_{}.txt", i);
        let http_name = format!("http_{}.txt", i);
        assert!(stdout.contains(&api_name), "missing {}", api_name);
        assert!(stdout.contains(&http_name), "missing {}", http_name);
    }
    Ok(())
}
