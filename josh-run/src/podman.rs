use anyhow::Context;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::meta::NetworkMode;

pub struct PodmanRunArgs {
    pub image: String,
    pub entrypoint: String,
    pub command: Vec<String>,
    pub volumes: Vec<(String, String, bool)>, // (vol_name_or_path, mount_path, read_only)
    pub env_vars: Vec<(String, String)>,
    pub user: Option<String>,
    pub network: NetworkMode,
    pub workdir: Option<String>,
    pub rm: bool,
}

pub fn image_exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("podman")
        .args(["image", "inspect", "--format", "{{.Id}}", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run podman image inspect")?;
    Ok(status.success())
}

pub fn volume_exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("podman")
        .args(["volume", "inspect", "--format", "{{.Name}}", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run podman volume inspect")?;
    Ok(status.success())
}

pub fn volume_create(name: &str) -> anyhow::Result<()> {
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

pub fn ensure_volume(name: &str) -> anyhow::Result<()> {
    if volume_exists(name)? {
        return Ok(());
    }
    volume_create(name)?;
    if !volume_exists(name)? {
        anyhow::bail!("podman volume {name} was not created");
    }
    Ok(())
}

pub fn volume_rm(name: &str) -> anyhow::Result<()> {
    volume_rm_with_args(name, false)
}

pub fn volume_rm_force(name: &str) -> anyhow::Result<()> {
    volume_rm_with_args(name, true)
}

fn volume_rm_with_args(name: &str, force: bool) -> anyhow::Result<()> {
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

pub fn recreate_volume(name: &str) -> anyhow::Result<()> {
    if volume_exists(name)? {
        volume_rm_force(name)?;
        if volume_exists(name)? {
            anyhow::bail!("podman volume {name} still exists after removal");
        }
    }
    volume_create(name)?;
    if !volume_exists(name)? {
        anyhow::bail!("podman volume {name} was not created");
    }
    Ok(())
}

pub fn volume_import(name: &str, tar_data: &[u8]) -> anyhow::Result<()> {
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
        .write_all(tar_data)
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

pub fn volume_export(name: &str) -> anyhow::Result<Vec<u8>> {
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

pub fn list_images_with_prefix(prefix: &str) -> anyhow::Result<Vec<String>> {
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

pub fn list_volumes_with_prefix(prefix: &str) -> anyhow::Result<Vec<String>> {
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

pub fn rmi(name: &str) -> anyhow::Result<()> {
    let output = Command::new("podman")
        .args(["rmi", "--force", name])
        .output()
        .context("failed to run podman rmi")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("podman rmi {name} failed: {stderr}");
    }
    Ok(())
}

pub struct RunOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run a container. Streams stdout/stderr to the terminal in real-time while
/// also capturing them. Returns exit code and captured output.
pub fn run(args: PodmanRunArgs) -> anyhow::Result<RunOutput> {
    let mut cmd = Command::new("podman");
    cmd.arg("run");

    if args.rm {
        cmd.arg("--rm");
    }

    if let Some(user) = &args.user {
        cmd.args(["--user", user]);
    }

    match args.network {
        NetworkMode::Host => {
            cmd.args(["--network", "host"]);
        }
        NetworkMode::None => {
            cmd.args(["--network", "none"]);
        }
    }

    if let Some(workdir) = &args.workdir {
        cmd.args(["--workdir", workdir]);
    }

    for (vol, mount, read_only) in &args.volumes {
        let spec = if *read_only {
            format!("{vol}:{mount}:ro")
        } else {
            format!("{vol}:{mount}")
        };
        cmd.args(["--volume", &spec]);
    }

    for (key, val) in &args.env_vars {
        cmd.args(["-e", &format!("{key}={val}")]);
    }

    cmd.args(["--entrypoint", &args.entrypoint]);
    cmd.arg(&args.image);
    cmd.args(&args.command);

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to run podman container")?;

    let child_stdout = child.stdout.take().expect("stdout piped");
    let child_stderr = child.stderr.take().expect("stderr piped");

    // Tee stdout: forward to parent stdout in real-time and collect into buffer.
    let stdout_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut reader = child_stdout;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::Write::write_all(&mut std::io::stdout(), &chunk[..n]);
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
        buf
    });

    // Tee stderr: forward to parent stderr in real-time and collect into buffer.
    let stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut reader = child_stderr;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::Write::write_all(&mut std::io::stderr(), &chunk[..n]);
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
        buf
    });

    let status = child
        .wait()
        .context("failed to wait for podman container")?;
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Ok(RunOutput {
        exit_code: status.code().unwrap_or(1),
        stdout,
        stderr,
    })
}
