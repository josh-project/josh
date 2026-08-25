//! Podman implementation of the backend capability traits.
//!
//! Each capability delegates to the thematic submodule that owns its Podman
//! commands: [`envs`], [`artifacts`], [`run`], or [`sidecars`].
//!
//! Container specifics the scheduler is unaware of live here: the internal sidecar
//! network, the `busybox` chown used for ownership fix-ups, and the detached
//! containers that realize sidecar workers.

use anyhow::Context;
use std::process::Command;

mod artifacts;
mod envs;
mod run;
mod sidecars;

/// Internal network sidecar workers and their consuming steps are attached to.
const SIDECAR_NETWORK: &str = "josh-sidecar-net";

/// Podman container runtime backend.
pub struct PodmanRuntime;

impl PodmanRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PodmanRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// --- shared private helpers (used by more than one submodule) ---

/// Host uid/gid of the invoking user — the identity container steps run as and
/// artifacts are chowned to. This is a container mechanic; the scheduler never
/// needs to know it.
#[cfg(unix)]
fn host_uid_gid() -> (u32, u32) {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    (uid, gid)
}

/// Unreachable: `josh compose` is refused on Windows before any container runs.
#[cfg(windows)]
fn host_uid_gid() -> (u32, u32) {
    unreachable!("josh compose is not supported on Windows")
}

fn host_identity() -> String {
    let (uid, gid) = host_uid_gid();
    format!("{uid}:{gid}")
}

/// Chown an artifact's contents to the invoking user via a throwaway busybox
/// container. The mount path is arbitrary — only the contents matter.
fn align_artifact(artifact: &str) -> anyhow::Result<()> {
    let mount = "/mnt";
    let identity = host_identity();
    let status = Command::new("podman")
        .args([
            "run",
            "--rm",
            "--volume",
            &format!("{artifact}:{mount}"),
            "busybox",
            "chown",
            "-R",
            &identity,
            mount,
        ])
        .status()
        .context("failed to run chown container")?;
    if !status.success() {
        anyhow::bail!("chown failed for artifact {artifact}");
    }
    Ok(())
}
