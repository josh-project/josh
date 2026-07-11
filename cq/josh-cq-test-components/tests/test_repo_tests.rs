use anyhow::{Context, bail};
use josh_cq_test_components::{TestRepo, TreeEntry, TreeMode};
use tokio::process::Command;

static GIT_ENV: &[(&str, &str)] = &[
    ("GIT_AUTHOR_NAME", "test"),
    ("GIT_AUTHOR_EMAIL", "test@test.com"),
    ("GIT_COMMITTER_NAME", "test"),
    ("GIT_COMMITTER_EMAIL", "test@test.com"),
];

async fn run_git(
    args: &[&str],
    dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
) -> anyhow::Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    for (k, v) in GIT_ENV {
        cmd.env(k, v);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.output().await.context("failed to run git")
}

fn output_to_string(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn check_git_ok(output: &std::process::Output, args: &[&str]) -> anyhow::Result<()> {
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

    let url = repo.url().to_string();
    let output = run_git(&["ls-remote", &url], None, &[]).await?;
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
    let dir_env = git_dir_env(repo.path())?;
    let output = run_git(
        &["ls-tree", "-r", &oid_str],
        None,
        &[(dir_env.0.as_str(), dir_env.1.as_str())],
    )
    .await?;
    check_git_ok(&output, &["ls-tree"])?;
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
    let head_str = head.to_string();
    let dir_env = git_dir_env(repo.path())?;
    let output = run_git(
        &["ls-tree", "-r", &head_str],
        None,
        &[(dir_env.0.as_str(), dir_env.1.as_str())],
    )
    .await?;
    check_git_ok(&output, &["ls-tree"])?;
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
    let head_str = head.to_string();
    let dir_env = git_dir_env(repo.path())?;
    let output = run_git(
        &["ls-tree", "-r", &head_str],
        None,
        &[(dir_env.0.as_str(), dir_env.1.as_str())],
    )
    .await?;
    check_git_ok(&output, &["ls-tree"])?;
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

    let output = run_git(&["clone", &url, "."], Some(clone_path.as_path()), &[]).await?;
    check_git_ok(&output, &["clone"])?;

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
    run_git(&["clone", &url, "."], Some(clone_path.as_path()), &[]).await?;

    // Make change and push
    std::fs::write(clone_path.join("b.txt"), "pushed")?;
    run_git(&["add", "b.txt"], Some(clone_path.as_path()), &[]).await?;
    run_git(
        &["commit", "-m", "push commit"],
        Some(clone_path.as_path()),
        &[],
    )
    .await?;
    run_git(&["push", "origin", "main"], Some(clone_path.as_path()), &[]).await?;

    let head = repo.get_head("refs/heads/main").await?;
    let head_str = head.to_string();
    let dir_env = git_dir_env(repo.path())?;
    let output = run_git(
        &["ls-tree", "-r", &head_str],
        None,
        &[(&dir_env.0, &dir_env.1)],
    )
    .await?;
    check_git_ok(&output, &["ls-tree"])?;

    let stdout = output_to_string(&output);
    assert!(stdout.contains("a.txt"), "should have a.txt: {}", stdout);
    assert!(stdout.contains("b.txt"), "should have b.txt: {}", stdout);
    Ok(())
}
