use anyhow::Context;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::meta;
use crate::podman;

/// Ensure the podman image for the given build tree exists, building it if needed.
/// Returns the image name.
pub fn ensure_image(repo: &git2::Repository, build_tree: git2::Oid) -> anyhow::Result<String> {
    let image_name = format!("ws_image_{build_tree}");

    if podman::image_exists(&image_name)? {
        eprintln!("[image:{image_name}] Already built");
        return Ok(image_name);
    }

    eprintln!("[image:{image_name}] Building...");

    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };

    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };

    // Process bases/ subtree: build base images first
    let base_entries = meta::read_blob_entries(repo, build_tree, "bases");
    let mut base_build_args: Vec<String> = vec![];
    for (base_name, base_sha) in &base_entries {
        let base_oid = git2::Oid::from_str(base_sha.trim())
            .with_context(|| format!("invalid base SHA for {base_name}: {base_sha}"))?;
        let base_image = ensure_image(repo, base_oid)?;
        base_build_args.push(format!("--build-arg={base_name}={base_image}"));
    }

    // Custom build args from args/ subtree
    let custom_args = meta::read_blob_entries(repo, build_tree, "args");
    let custom_build_args: Vec<String> = custom_args
        .into_iter()
        .map(|(k, v)| format!("--build-arg={k}={v}"))
        .collect();

    // Get context tree OID
    let tree = repo.find_tree(build_tree)?;
    let context_entry = tree
        .get_path(std::path::Path::new("context"))
        .context("workspace image tree missing 'context' subtree")?;
    let context_oid = context_entry.id();

    let tar_data = crate::archive::tree_to_tar(repo, context_oid)?;

    let mut podman_cmd = Command::new("podman");
    podman_cmd.args(["build", "--format=docker"]);
    podman_cmd.args(["--build-arg", &format!("ARCH={arch}")]);
    podman_cmd.args(["--build-arg", &format!("USER_UID={uid}")]);
    podman_cmd.args(["--build-arg", &format!("USER_GID={gid}")]);

    for arg in &base_build_args {
        podman_cmd.arg(arg);
    }
    for arg in &custom_build_args {
        podman_cmd.arg(arg);
    }

    podman_cmd.args(["-t", &image_name]);
    podman_cmd.arg("-"); // read build context from stdin
    podman_cmd.stdin(Stdio::piped());

    let mut podman_child = podman_cmd.spawn().context("failed to spawn podman build")?;
    if let Some(mut stdin) = podman_child.stdin.take() {
        stdin
            .write_all(&tar_data)
            .context("failed to write tar to podman build stdin")?;
    }
    let status = podman_child
        .wait()
        .context("failed to wait for podman build")?;

    if !status.success() {
        anyhow::bail!("podman build failed for image {image_name}");
    }

    eprintln!("[image:{image_name}] Built successfully");
    Ok(image_name)
}
