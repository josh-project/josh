use anyhow::Context;
use std::io::Write;
use std::process::{Command, Stdio};

use super::{DockerRuntime, host_uid_gid};
use josh_compose_backend::{EnvRecipe, EnvironmentBackend};

fn env_exists(key: &str) -> anyhow::Result<bool> {
    let status = Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", key])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run docker image inspect")?;
    Ok(status.success())
}

fn prepare_env(key: &str, recipe: EnvRecipe) -> anyhow::Result<()> {
    // Standard build args a Containerfile expects: the target architecture
    // (Go-style naming) and the host UID/GID so images can match the invoking
    // user. These are container-build concerns, so the backend owns them —
    // the scheduler only supplies the logical build args in `recipe`.
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let (uid, gid) = host_uid_gid();

    let mut cmd = Command::new("docker");
    cmd.arg("build");
    cmd.args(["--build-arg", &format!("ARCH={arch}")]);
    cmd.args(["--build-arg", &format!("USER_UID={uid}")]);
    cmd.args(["--build-arg", &format!("USER_GID={gid}")]);
    for (k, v) in &recipe.build_args {
        cmd.arg(format!("--build-arg={k}={v}"));
    }
    cmd.args(["-t", key]);
    cmd.arg("-"); // read build context from stdin
    cmd.stdin(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn docker build")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&recipe.context)
            .context("failed to write tar to docker build stdin")?;
    }
    let status = child.wait().context("failed to wait for docker build")?;
    if !status.success() {
        anyhow::bail!("docker build failed for environment {key}");
    }
    Ok(())
}

fn list_envs(prefix: &str) -> anyhow::Result<Vec<String>> {
    let output = Command::new("docker")
        .args([
            "images",
            "--format",
            "{{.Repository}}:{{.Tag}}",
            "--filter",
            &format!("reference={prefix}*"),
        ])
        .output()
        .context("failed to run docker images")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn remove_env(key: &str) -> anyhow::Result<()> {
    let output = Command::new("docker")
        .args(["rmi", "--force", key])
        .output()
        .context("failed to run docker rmi")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("docker rmi {key} failed: {stderr}");
    }
    Ok(())
}

impl EnvironmentBackend for DockerRuntime {
    fn env_exists(&self, key: &str) -> anyhow::Result<bool> {
        env_exists(key)
    }

    fn prepare_env(&self, key: &str, recipe: EnvRecipe) -> anyhow::Result<()> {
        prepare_env(key, recipe)
    }

    fn list_envs(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        list_envs(prefix)
    }

    fn remove_env(&self, key: &str) -> anyhow::Result<()> {
        remove_env(key)
    }
}
