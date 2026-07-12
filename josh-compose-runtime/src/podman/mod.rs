//! Podman backend for [`crate::Runtime`].
//!
//! Each trait method wraps the corresponding `podman` CLI invocation. The single
//! `impl Runtime for PodmanRuntime` below is a thin routing table; the real work
//! lives in the thematic submodules — [`envs`], [`artifacts`], [`run`], and
//! [`sidecars`] — each holding the bodies as free functions. (Rust forbids
//! splitting a trait impl across files, so the impl stays in one place and
//! delegates.)
//!
//! Container specifics the scheduler is unaware of live here: the internal sidecar
//! network, the `busybox` chown used for ownership fix-ups, and the detached
//! containers that realize sidecar workers.

use anyhow::Context;
use std::process::Command;

use crate::{EnvRecipe, RunArgs, RunOutput, Runtime, SidecarArgs, SidecarHandle};

mod artifacts;
mod envs;
mod run;
mod sidecars;

/// Internal network sidecar workers and their consuming steps are attached to.
pub(super) const SIDECAR_NETWORK: &str = "josh-sidecar-net";

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

impl Runtime for PodmanRuntime {
    fn env_exists(&self, key: &str) -> anyhow::Result<bool> {
        envs::env_exists(key)
    }
    fn prepare_env(&self, key: &str, recipe: EnvRecipe) -> anyhow::Result<()> {
        envs::prepare_env(key, recipe)
    }
    fn list_envs(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        envs::list_envs(prefix)
    }
    fn remove_env(&self, key: &str) -> anyhow::Result<()> {
        envs::remove_env(key)
    }

    fn artifact_exists(&self, name: &str) -> anyhow::Result<bool> {
        artifacts::artifact_exists(name)
    }
    fn create_artifact(&self, name: &str) -> anyhow::Result<()> {
        artifacts::create_artifact(name)
    }
    fn import_artifact(&self, name: &str, tar: &[u8]) -> anyhow::Result<()> {
        artifacts::import_artifact(name, tar)
    }
    fn export_artifact(&self, name: &str) -> anyhow::Result<Vec<u8>> {
        artifacts::export_artifact(name)
    }
    fn extract_artifact(&self, name: &str, dest: &std::path::Path) -> anyhow::Result<()> {
        artifacts::extract_artifact(name, dest)
    }
    fn remove_artifact(&self, name: &str, force: bool) -> anyhow::Result<()> {
        artifacts::remove_artifact(name, force)
    }
    fn list_artifacts(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        artifacts::list_artifacts(prefix)
    }
    fn create_scratch_artifact(&self, tar: &[u8]) -> anyhow::Result<String> {
        artifacts::create_scratch_artifact(tar)
    }
    fn recreate_artifact(&self, name: &str) -> anyhow::Result<()> {
        artifacts::recreate_artifact(name)
    }

    fn run(&self, args: RunArgs) -> anyhow::Result<RunOutput> {
        run::run(args)
    }

    fn start_sidecar(&self, args: SidecarArgs) -> anyhow::Result<SidecarHandle> {
        sidecars::start_sidecar(args)
    }
    fn stop_sidecar(&self, handle: &SidecarHandle) -> anyhow::Result<()> {
        sidecars::stop_sidecar(handle)
    }
}

// --- shared private helpers (used by more than one submodule) ---

/// Host uid/gid of the invoking user — the identity container steps run as and
/// artifacts are chowned to. This is a container mechanic; the scheduler never
/// needs to know it.
pub(super) fn host_uid_gid() -> (u32, u32) {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    (uid, gid)
}

pub(super) fn host_identity() -> String {
    let (uid, gid) = host_uid_gid();
    format!("{uid}:{gid}")
}

/// Chown an artifact's contents to the invoking user via a throwaway busybox
/// container. The mount path is arbitrary — only the contents matter.
pub(super) fn align_artifact(artifact: &str) -> anyhow::Result<()> {
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
