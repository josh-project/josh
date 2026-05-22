use anyhow::Context;
use std::collections::HashSet;
use std::process::Command;

use crate::image;
use crate::job_cache;
use crate::meta::{self, NetworkMode, OutputMode};
use crate::podman::{self, PodmanRunArgs};

const SIDECAR_NETWORK: &str = "josh-sccache-net";
const SIDECAR_IP_PLACEHOLDER: &str = "{SIDECAR_IP}";

/// Read env var names from `sidecar_passthrough` (keys only) and look each up in the
/// outer process environment. Returns `None` if any listed variable is absent or empty.
fn resolve_passthrough(passthrough: &[(String, String)]) -> Option<Vec<(String, String)>> {
    passthrough
        .iter()
        .map(|(name, _)| {
            let val = std::env::var(name).unwrap_or_default();
            if val.is_empty() {
                None
            } else {
                Some((name.clone(), val))
            }
        })
        .collect()
}

/// Start the sccache proxy sidecar. Returns the container name.
fn start_sccache_proxy(
    repo: &git2::Repository,
    proxy_image_oid: git2::Oid,
    sidecar_env: &[(String, String)],
    passthrough_env: Vec<(String, String)>,
) -> anyhow::Result<String> {
    let image_name = image::ensure_image(repo, proxy_image_oid)?;
    podman::ensure_network_internal(SIDECAR_NETWORK)?;

    let bytes: [u8; 4] = rand::random();
    let container_name = format!("josh-sccache-proxy-{}", hex::encode(bytes));

    let mut env_vars: Vec<(String, String)> = sidecar_env.to_vec();
    env_vars.extend(passthrough_env);

    podman::run_detached(podman::PodmanRunDetachedArgs {
        image: image_name,
        name: container_name.clone(),
        networks: vec!["bridge".to_string(), SIDECAR_NETWORK.to_string()],
        env_vars,
    })?;

    // Give the proxy a moment to start listening.
    std::thread::sleep(std::time::Duration::from_millis(500));

    Ok(container_name)
}

fn stop_sccache_proxy(container_name: &str) {
    let _ = podman::stop_container(container_name);
    let _ = podman::rm_container_force(container_name);
}

/// Run a container workspace identified by `ws_tree`.
/// Uses the output cache if available.
/// `attempted` tracks ws_trees already tried in this invocation to avoid redundant re-runs.
pub fn run_container(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    sidecar_image: Option<git2::Oid>,
    sidecar_env: &[(String, String)],
    sidecar_passthrough: &[(String, String)],
    sidecar_inject: &[(String, String)],
    attempted: &mut HashSet<git2::Oid>,
) -> anyhow::Result<()> {
    let workspace_meta = meta::read_meta(repo, ws_tree)?;

    // Cache check: skip only if a previous successful run is recorded.
    let hash = ws_tree.to_string();
    let out_vol = format!("out_{ws_tree}");
    if job_cache::is_cached_success(&hash) {
        eprintln!(
            "[{}] Using cached output ({})",
            workspace_meta.label, ws_tree
        );
        return Ok(());
    }

    // Dedup check: if we already attempted this ws_tree in this invocation, don't retry.
    if !attempted.insert(ws_tree) {
        anyhow::bail!(
            "[{}] Already attempted in this run ({})",
            workspace_meta.label,
            ws_tree
        );
    }

    eprintln!("[{}] Running ({})", workspace_meta.label, ws_tree);

    // Run all dependencies, collecting failures so sibling jobs still get a chance to run.
    let input_entries = meta::read_blob_entries(repo, ws_tree, "inputs");
    let mut dep_volumes: Vec<(String, String, bool)> = vec![];
    let mut dep_errors: Vec<String> = vec![];
    for (dep_name, dep_sha) in &input_entries {
        let dep_tree = match git2::Oid::from_str(dep_sha.trim()) {
            Ok(oid) => oid,
            Err(_) => {
                dep_errors.push(format!("dependency {dep_name}: invalid SHA {dep_sha:?}"));
                continue;
            }
        };
        if let Err(e) = run_container(
            repo,
            dep_tree,
            sidecar_image,
            sidecar_env,
            sidecar_passthrough,
            sidecar_inject,
            attempted,
        ) {
            dep_errors.push(format!("dependency {dep_name} failed: {e}"));
            continue;
        }
        let dep_out_vol = format!("out_{dep_tree}");
        if !podman::volume_exists(&dep_out_vol)? {
            dep_errors.push(format!(
                "dependency {dep_name} has no output volume (output must not be 'none')"
            ));
            continue;
        }
        dep_volumes.push((dep_out_vol, format!("/{dep_name}"), true));
    }
    if !dep_errors.is_empty() {
        anyhow::bail!("{}", dep_errors.join("\n"));
    }

    // If there's no image, this is an orchestrator workspace — deps are all we run.
    let Some(image_oid) = workspace_meta.image else {
        job_cache::write_result(&hash, true, &[], &[]);
        eprintln!("[{}] Done (orchestrator)", workspace_meta.label);
        return Ok(());
    };
    let Some(worktree_oid) = workspace_meta.worktree else {
        job_cache::write_result(&hash, true, &[], &[]);
        eprintln!("[{}] Done (no worktree)", workspace_meta.label);
        return Ok(());
    };

    // Build the container image
    let image_name = image::ensure_image(repo, image_oid)?;

    // Cache volume setup
    let mut cache_volume: Option<String> = None;
    if let Some(cache_name) = &workspace_meta.cache {
        let vol_name = format!("{cache_name}_cache");
        podman::ensure_volume(&vol_name)?;
        cache_volume = Some(vol_name);
    }

    // Read env vars from env/ subtree
    let mut env_vars = meta::read_blob_entries(repo, ws_tree, "env");

    // Resolve network mode: Sidecar starts a proxy and injects env vars.
    // `proxy_container` holds the name of a running proxy that must be stopped after the build.
    let mut proxy_container: Option<String> = None;
    let network = match workspace_meta.network {
        NetworkMode::Sidecar => match (sidecar_image, resolve_passthrough(sidecar_passthrough)) {
            (Some(img_oid), Some(pass_env)) if !sidecar_env.is_empty() => {
                match start_sccache_proxy(repo, img_oid, sidecar_env, pass_env) {
                    Ok(proxy_name) => match podman::container_ip(&proxy_name, SIDECAR_NETWORK) {
                        Ok(proxy_ip) => {
                            let injected: Vec<(String, String)> = sidecar_inject
                                .iter()
                                .map(|(k, v)| {
                                    (k.clone(), v.replace(SIDECAR_IP_PLACEHOLDER, &proxy_ip))
                                })
                                .collect();
                            env_vars.extend(injected);
                            proxy_container = Some(proxy_name);
                            NetworkMode::Named(SIDECAR_NETWORK.to_string())
                        }
                        Err(e) => {
                            eprintln!(
                                "[{}] Sidecar: failed to get proxy IP: {e}; \
                                         using local sccache only",
                                workspace_meta.label
                            );
                            stop_sccache_proxy(&proxy_name);
                            NetworkMode::None
                        }
                    },
                    Err(e) => {
                        eprintln!(
                            "[{}] Sidecar: failed to start proxy: {e}; \
                                 using local sccache only",
                            workspace_meta.label
                        );
                        NetworkMode::None
                    }
                }
            }
            _ => {
                eprintln!(
                    "[{}] Sidecar: missing image, credentials, or config; \
                         using local sccache only",
                    workspace_meta.label
                );
                NetworkMode::None
            }
        },
        other => other,
    };
    // Ensure the proxy is stopped when this function returns, whether success or failure.
    let _proxy_cleanup = defer::defer(move || {
        if let Some(name) = proxy_container {
            stop_sccache_proxy(&name);
        }
    });

    // Create ephemeral snapshot volume from worktree
    let snapshot_vol = {
        let random_hex = {
            let bytes: [u8; 4] = rand::random();
            hex::encode(bytes)
        };
        format!("snapshot_{}_{}", ws_tree, random_hex)
    };
    podman::volume_create(&snapshot_vol)?;

    // Cleanup snapshot volume on exit (even on error)
    let snapshot_vol_clone = snapshot_vol.clone();
    let _cleanup = defer::defer(move || {
        let _ = podman::volume_rm(&snapshot_vol_clone);
    });

    // Import worktree contents into snapshot volume via git archive
    let tar_data = git_archive_tree(repo, worktree_oid)?;
    podman::volume_import(&snapshot_vol, &tar_data)?;

    // Fix ownership to match host user inside the snapshot and output volumes
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let user_str = format!("{uid}:{gid}");

    // Assemble all volumes for the run
    let workdir = format!("/{}", snapshot_vol);
    let mut volumes: Vec<(String, String, bool)> = vec![];
    volumes.push((snapshot_vol.clone(), workdir.clone(), false));

    if workspace_meta.output != OutputMode::None {
        podman::recreate_volume(&out_vol)?;
        chown_volume("busybox", &out_vol, "/out", &user_str)?;
        volumes.push((out_vol.clone(), "/out".to_string(), false));
    }

    for (dep_vol, mount, ro) in &dep_volumes {
        chown_volume("busybox", dep_vol, mount, &user_str)?;
        volumes.push((dep_vol.clone(), mount.clone(), *ro));
    }

    if let Some(cache_vol) = &cache_volume {
        volumes.push((cache_vol.clone(), "/opt/cache".to_string(), false));
    }

    chown_volume("busybox", &snapshot_vol, &workdir, &user_str)?;

    // Run the container
    let output = podman::run(PodmanRunArgs {
        image: image_name,
        entrypoint: "sh".to_string(),
        command: vec!["-c".to_string(), workspace_meta.cmd.clone()],
        volumes,
        env_vars,
        user: Some(user_str),
        network,
        workdir: Some(workdir),
        rm: true,
    })?;

    let success = output.exit_code == 0;
    job_cache::write_result(&hash, success, &output.stdout, &output.stderr);

    if workspace_meta.output == OutputMode::Workdir {
        let tar_data = podman::volume_export(&out_vol)?;
        let mut archive = tar::Archive::new(std::io::Cursor::new(tar_data));
        archive
            .unpack(".")
            .map_err(|e| anyhow::anyhow!("failed to extract output volume: {e}"))?;
    }

    if !success {
        anyhow::bail!(
            "[{}] FAILED with exit code {}",
            workspace_meta.label,
            output.exit_code
        );
    }

    eprintln!("[{}] SUCCESS", workspace_meta.label);
    Ok(())
}

fn git_archive_tree(repo: &git2::Repository, tree_oid: git2::Oid) -> anyhow::Result<Vec<u8>> {
    crate::archive::tree_to_tar(repo, tree_oid)
}

/// Run busybox to chown a volume mount path to the given user:group.
fn chown_volume(image: &str, vol_name: &str, mount_path: &str, user: &str) -> anyhow::Result<()> {
    let status = Command::new("podman")
        .args([
            "run",
            "--rm",
            "--volume",
            &format!("{vol_name}:{mount_path}"),
            image,
            "chown",
            "-R",
            user,
            mount_path,
        ])
        .status()
        .context("failed to run chown container")?;

    if !status.success() {
        anyhow::bail!("chown failed for volume {vol_name}");
    }

    Ok(())
}
