use anyhow::Context;
use backon::{BlockingRetryable, ExponentialBuilder};
use std::collections::HashSet;
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use crate::image;
use crate::job_cache;
use crate::meta::{self, NetworkMode, OutputMode, SidecarSpec};
use crate::podman::{self, PodmanRunArgs};

const SIDECAR_NETWORK: &str = "josh-sidecar-net";
const SIDECAR_IP_PLACEHOLDER: &str = "{SIDECAR_IP}";

/// Resolve passthrough env names by looking each up in the outer process environment.
/// Errors listing every missing variable when any are absent or empty, so that the
/// developer sees the full set of misconfigured env vars in one go (locally and in CI).
fn resolve_passthrough(
    sidecar_name: &str,
    passthrough: &[(String, String)],
) -> anyhow::Result<Vec<(String, String)>> {
    let mut resolved = vec![];
    let mut missing = vec![];
    for (name, _) in passthrough {
        let val = std::env::var(name).unwrap_or_default();
        if val.is_empty() {
            missing.push(name.clone());
        } else {
            resolved.push((name.clone(), val));
        }
    }
    if !missing.is_empty() {
        anyhow::bail!(
            "sidecar {sidecar_name}: missing required passthrough env vars: {}",
            missing.join(", ")
        );
    }
    Ok(resolved)
}

/// Start a sidecar container on the shared internal network. Returns `Ok((name, ip))` once the
/// sidecar is accepting TCP connections on `spec.port`. Any failure — missing passthrough
/// credentials, image build, container start, IP lookup, or readiness-probe timeout — is a hard
/// error; there is no soft-skip path.
fn start_sidecar(repo: &git2::Repository, spec: &SidecarSpec) -> anyhow::Result<(String, String)> {
    let image_name = image::ensure_image(repo, spec.image)?;

    let passthrough_env = resolve_passthrough(&spec.name, &spec.passthrough)?;

    podman::ensure_network_internal(SIDECAR_NETWORK)?;

    let bytes: [u8; 4] = rand::random();
    let container_name = format!("josh-sidecar-{}-{}", spec.name, hex::encode(bytes));

    let mut env_vars: Vec<(String, String)> = spec.env.clone();
    env_vars.extend(passthrough_env);

    podman::run_detached(podman::PodmanRunDetachedArgs {
        image: image_name,
        name: container_name.clone(),
        networks: vec!["bridge".to_string(), SIDECAR_NETWORK.to_string()],
        env_vars,
        port: spec.port,
    })?;

    let sidecar_ip = match podman::container_ip(&container_name, SIDECAR_NETWORK) {
        Ok(ip) => ip,
        Err(e) => {
            stop_sidecar(&container_name);
            return Err(e);
        }
    };

    let host_addr =
        podman::container_port(&container_name, spec.port).context("sidecar port not published")?;

    let probe = || -> std::io::Result<()> {
        TcpStream::connect_timeout(&host_addr, Duration::from_millis(200)).map(|_| ())
    };

    let backoff = ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(25))
        .with_max_delay(Duration::from_millis(250))
        .with_max_times(50)
        .with_jitter();

    if let Err(e) = probe.retry(backoff).call() {
        stop_sidecar(&container_name);
        anyhow::bail!(
            "sidecar {} not reachable at {host_addr} within timeout: {e}",
            spec.name
        );
    }

    Ok((container_name, sidecar_ip))
}

fn stop_sidecar(container_name: &str) {
    let _ = podman::stop_container(container_name);
    let _ = podman::rm_container_force(container_name);
}

/// Run a container workspace identified by `ws_tree`.
/// Uses the output cache if available.
/// `attempted` tracks ws_trees already tried in this invocation to avoid redundant re-runs.
pub fn run_container(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    attempted: &mut HashSet<git2::Oid>,
    extract_to_workdir: bool,
) -> anyhow::Result<()> {
    let workspace_meta = meta::read_meta(repo, ws_tree)?;

    // Cache check: skip only if a previous successful run is recorded AND its
    // output volume is still present (when one is expected). Mirrors
    // `plan::workspace_is_skippable` so a stale marker without its volume —
    // reachable when an R2 volume pull fails after the marker pull succeeded —
    // self-heals by re-running rather than failing downstream dep-mounts.
    let hash = ws_tree.to_string();
    let out_vol = format!("out_{ws_tree}");
    if job_cache::is_cached_success(&hash)
        && (workspace_meta.output == OutputMode::None || podman::volume_exists(&out_vol)?)
    {
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
        if let Err(e) = run_container(repo, dep_tree, attempted, extract_to_workdir) {
            dep_errors.push(format!("dependency {dep_name} failed: {e}"));
            continue;
        }
        let dep_meta = meta::read_meta(repo, dep_tree)?;
        if dep_meta.output == OutputMode::None {
            continue;
        }
        let dep_out_vol = format!("out_{dep_tree}");
        if !podman::volume_exists(&dep_out_vol)? {
            dep_errors.push(format!("dependency {dep_name} has no output volume"));
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

    // Start any declared sidecars and inject their IPs into the main container's env.
    // Any sidecar failure (missing creds, start error, readiness timeout) is fatal: tear
    // down already-started sidecars and bail so the misconfiguration surfaces equally
    // on dev machines and in CI.
    let mut started_sidecars: Vec<String> = vec![];
    let network = if workspace_meta.sidecars.is_empty() {
        workspace_meta.network.clone()
    } else {
        for spec in &workspace_meta.sidecars {
            match start_sidecar(repo, spec) {
                Ok((name, ip)) => {
                    started_sidecars.push(name);
                    for (k, v) in &spec.inject {
                        env_vars.push((k.clone(), v.replace(SIDECAR_IP_PLACEHOLDER, &ip)));
                    }
                }
                Err(e) => {
                    for name in &started_sidecars {
                        stop_sidecar(name);
                    }
                    anyhow::bail!(
                        "[{}] sidecar '{}' failed to start: {e}",
                        workspace_meta.label,
                        spec.name
                    );
                }
            }
        }
        NetworkMode::Named(SIDECAR_NETWORK.to_string())
    };
    // Ensure all started sidecars are stopped when this function returns.
    let sidecars_for_cleanup = std::mem::take(&mut started_sidecars);
    let _sidecar_cleanup = defer::defer(move || {
        for name in &sidecars_for_cleanup {
            stop_sidecar(name);
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
    let workdir = "/worktree";
    let mut volumes: Vec<(String, String, bool)> = vec![];
    volumes.push((snapshot_vol.clone(), workdir.to_string(), false));

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
        workdir: Some(workdir.to_string()),
        rm: true,
    })?;

    let success = output.exit_code == 0;
    job_cache::write_result(&hash, success, &output.stdout, &output.stderr);

    if workspace_meta.output == OutputMode::Workdir && extract_to_workdir {
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
