use anyhow::Context;
use std::io::Write;
use std::process::{Command, Stdio};

use super::align_artifact;

pub(super) fn artifact_exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("podman")
        .args(["volume", "inspect", "--format", "{{.Name}}", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run podman volume inspect")?;
    Ok(status.success())
}

pub(super) fn create_artifact(name: &str) -> anyhow::Result<()> {
    let output = Command::new("podman")
        .args(["volume", "create", name])
        .output()
        .context("failed to run podman volume create")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman volume create {name} failed: {stderr}");
    }
    Ok(())
}

pub(super) fn import_artifact(name: &str, tar: &[u8]) -> anyhow::Result<()> {
    let mut child = Command::new("podman")
        .args(["volume", "import", name, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn podman volume import")?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(tar)
        .context("failed to write tar data to podman volume import")?;

    let output = child
        .wait_with_output()
        .context("failed to wait for podman volume import")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman volume import {name} failed: {stderr}");
    }
    Ok(())
}

pub(super) fn export_artifact(name: &str) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("podman")
        .args(["volume", "export", name])
        .output()
        .context("failed to run podman volume export")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman volume export {name} failed: {stderr}");
    }
    Ok(output.stdout)
}

pub(super) fn remove_artifact(name: &str, force: bool) -> anyhow::Result<()> {
    let mut args = vec!["volume", "rm"];
    if force {
        args.push("--force");
    }
    args.push(name);
    let output = Command::new("podman")
        .args(&args)
        .output()
        .context("failed to run podman volume rm")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman volume rm {name} failed: {stderr}");
    }
    Ok(())
}

pub(super) fn list_artifacts(prefix: &str) -> anyhow::Result<Vec<String>> {
    let output = Command::new("podman")
        .args(["volume", "ls", "--format", "{{.Name}}"])
        .output()
        .context("failed to run podman volume ls")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| l.starts_with(prefix))
        .collect())
}

pub(super) fn extract_artifact(name: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let tar_data = export_artifact(name)?;
    tar::Archive::new(std::io::Cursor::new(tar_data))
        .unpack(dest)
        .map_err(|e| anyhow::anyhow!("failed to extract artifact {name}: {e}"))
}

pub(super) fn create_scratch_artifact(tar: &[u8]) -> anyhow::Result<String> {
    let bytes: [u8; 4] = rand::random();
    let name = format!("josh-scratch-{}", hex::encode(bytes));
    create_artifact(&name)?;
    import_artifact(&name, tar)?;
    align_artifact(&name)?;
    Ok(name)
}

/// Override of the trait's default `recreate_artifact` to also fix ownership for
/// the invoking user (fresh podman volumes are root-owned).
pub(super) fn recreate_artifact(name: &str) -> anyhow::Result<()> {
    if artifact_exists(name)? {
        remove_artifact(name, true)?;
        if artifact_exists(name)? {
            anyhow::bail!("runtime artifact {name} still exists after removal");
        }
    }
    create_artifact(name)?;
    if !artifact_exists(name)? {
        anyhow::bail!("runtime artifact {name} was not created");
    }
    align_artifact(name)?;
    Ok(())
}
