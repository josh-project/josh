use anyhow::Context;
use std::io::Write;
use std::process::{Command, Stdio};

use super::{PodmanRuntime, align_artifact};
use josh_compose_backend::ArtifactBackend;

fn artifact_exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("podman")
        .args(["volume", "inspect", "--format", "{{.Name}}", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run podman volume inspect")?;
    Ok(status.success())
}

fn create_artifact(name: &str) -> anyhow::Result<()> {
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

fn import_artifact(name: &str, tar: &[u8]) -> anyhow::Result<()> {
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

fn export_artifact(name: &str) -> anyhow::Result<Vec<u8>> {
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

fn remove_artifact(name: &str, force: bool) -> anyhow::Result<()> {
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

fn list_artifacts(prefix: &str) -> anyhow::Result<Vec<String>> {
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

fn extract_artifact(name: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let tar_data = export_artifact(name)?;
    tar::Archive::new(std::io::Cursor::new(tar_data))
        .unpack(dest)
        .map_err(|e| anyhow::anyhow!("failed to extract artifact {name}: {e}"))
}

fn create_scratch_artifact(tar: &[u8]) -> anyhow::Result<String> {
    let bytes: [u8; 4] = rand::random();
    let name = format!("josh-scratch-{}", hex::encode(bytes));
    create_artifact(&name)?;
    if let Err(error) = import_artifact(&name, tar).and_then(|_| align_artifact(&name)) {
        if let Err(cleanup_error) = remove_artifact(&name, true) {
            return Err(error.context(format!(
                "failed to remove scratch artifact {name} after initialization failed: \
                 {cleanup_error}"
            )));
        }
        return Err(error);
    }
    Ok(name)
}

/// Override of the trait's default `recreate_artifact` to also fix ownership for
/// the invoking user (fresh podman volumes are root-owned).
fn recreate_artifact(name: &str) -> anyhow::Result<()> {
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

impl ArtifactBackend for PodmanRuntime {
    fn artifact_exists(&self, name: &str) -> anyhow::Result<bool> {
        artifact_exists(name)
    }

    fn create_artifact(&self, name: &str) -> anyhow::Result<()> {
        create_artifact(name)
    }

    fn import_artifact(&self, name: &str, tar: &[u8]) -> anyhow::Result<()> {
        import_artifact(name, tar)
    }

    fn export_artifact(&self, name: &str) -> anyhow::Result<Vec<u8>> {
        export_artifact(name)
    }

    fn extract_artifact(&self, name: &str, dest: &std::path::Path) -> anyhow::Result<()> {
        extract_artifact(name, dest)
    }

    fn remove_artifact(&self, name: &str, force: bool) -> anyhow::Result<()> {
        remove_artifact(name, force)
    }

    fn list_artifacts(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        list_artifacts(prefix)
    }

    fn create_scratch_artifact(&self, tar: &[u8]) -> anyhow::Result<String> {
        create_scratch_artifact(tar)
    }

    fn recreate_artifact(&self, name: &str) -> anyhow::Result<()> {
        recreate_artifact(name)
    }
}
