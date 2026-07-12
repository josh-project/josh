use anyhow::Context;
use std::io::Write;
use std::process::{Command, Stdio};

use super::host_uid_gid;
use crate::EnvRecipe;

pub(super) fn env_exists(key: &str) -> anyhow::Result<bool> {
    let status = Command::new("podman")
        .args(["image", "inspect", "--format", "{{.Id}}", key])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run podman image inspect")?;
    Ok(status.success())
}

pub(super) fn prepare_env(key: &str, recipe: EnvRecipe) -> anyhow::Result<()> {
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

    let mut cmd = Command::new("podman");
    cmd.args(["build", "--format=docker"]);
    cmd.args(["--build-arg", &format!("ARCH={arch}")]);
    cmd.args(["--build-arg", &format!("USER_UID={uid}")]);
    cmd.args(["--build-arg", &format!("USER_GID={gid}")]);
    for (k, v) in &recipe.build_args {
        cmd.arg(format!("--build-arg={k}={v}"));
    }
    cmd.args(["-t", key]);
    cmd.arg("-"); // read build context from stdin
    cmd.stdin(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn podman build")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&recipe.context)
            .context("failed to write tar to podman build stdin")?;
    }
    let status = child.wait().context("failed to wait for podman build")?;
    if !status.success() {
        anyhow::bail!("podman build failed for environment {key}");
    }
    Ok(())
}

pub(super) fn list_envs(prefix: &str) -> anyhow::Result<Vec<String>> {
    let output = Command::new("podman")
        .args([
            "images",
            "--format",
            "{{.Repository}}:{{.Tag}}",
            "--filter",
            &format!("reference={prefix}*"),
        ])
        .output()
        .context("failed to run podman images")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

pub(super) fn remove_env(key: &str) -> anyhow::Result<()> {
    let output = Command::new("podman")
        .args(["rmi", "--force", key])
        .output()
        .context("failed to run podman rmi")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("podman rmi {key} failed: {stderr}");
    }
    Ok(())
}
