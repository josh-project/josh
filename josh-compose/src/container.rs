use std::collections::HashSet;
use std::path::Path;

use josh_compose_runtime::{Mount, RunArgs, Runtime, SidecarArgs, SidecarHandle};

use crate::image;
use crate::job_cache;
use crate::meta::{self, OutputMode, SidecarSpec};
use crate::naming;

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

/// Start a sidecar worker via the runtime, which blocks until it is reachable.
/// Any failure — missing passthrough credentials, environment preparation, worker
/// start, or readiness timeout — is a hard error surfaced by the runtime; there is
/// no soft-skip path.
fn start_sidecar(
    repo: &git2::Repository,
    spec: &SidecarSpec,
    runtime: &dyn Runtime,
) -> anyhow::Result<SidecarHandle> {
    let env_key = image::ensure_image(repo, spec.image, runtime)?;

    let passthrough_env = resolve_passthrough(&spec.name, &spec.passthrough)?;

    let mut env_vars: Vec<(String, String)> = spec.env.clone();
    env_vars.extend(passthrough_env);

    runtime.start_sidecar(SidecarArgs {
        name: spec.name.clone(),
        env: env_key,
        port: spec.port,
        env_vars,
    })
}

/// Run a workspace identified by `ws_tree`, recursively running all dependencies
/// depth-first. Failures in sibling dependencies are collected so all are attempted
/// before bailing.
///
/// Sidecars declared in the workspace metadata are started before the main step and
/// torn down when this function returns (including on error paths), via [`defer::defer`].
/// A scratch artifact seeded from the worktree is created and cleaned up the same way.
///
/// `attempted` tracks ws_trees already visited in this invocation to avoid redundant
/// re-runs; the output cache may also short-circuit re-runs across invocations.
pub fn run_container(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    attempted: &mut HashSet<git2::Oid>,
    extract_to_workdir: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<()> {
    let workspace_meta = meta::read_meta(repo, ws_tree)?;

    // Cache check: skip only if a previous successful run is recorded AND its
    // output volume is still present (when one is expected). Mirrors
    // `plan::workspace_is_skippable` so a stale marker without its volume —
    // reachable when an R2 volume pull fails after the marker pull succeeded —
    // self-heals by re-running rather than failing downstream dep-mounts.
    let hash = ws_tree.to_string();
    let out_vol = naming::output(ws_tree);
    if job_cache::is_cached_success(&hash)
        && (workspace_meta.output == OutputMode::None || runtime.artifact_exists(&out_vol)?)
    {
        eprintln!(
            "[{}] Using cached output ({})",
            workspace_meta.label, ws_tree
        );
        return Ok(());
    }

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
        if let Err(e) = run_container(repo, dep_tree, attempted, extract_to_workdir, runtime) {
            dep_errors.push(format!("dependency {dep_name} failed: {e}"));
            continue;
        }
        let dep_meta = meta::read_meta(repo, dep_tree)?;
        if dep_meta.output == OutputMode::None {
            continue;
        }
        let dep_out_vol = naming::output(dep_tree);
        if !runtime.artifact_exists(&dep_out_vol)? {
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

    // Resolve the environment (cache-or-build).
    let image_name = image::ensure_image(repo, image_oid, runtime)?;

    let mut cache_volume: Option<String> = None;
    if let Some(cache_name) = &workspace_meta.cache {
        let vol_name = naming::cache(cache_name);
        runtime.ensure_artifact(&vol_name)?;
        cache_volume = Some(vol_name);
    }

    // Read env vars from env/ subtree
    let mut env_vars = meta::read_blob_entries(repo, ws_tree, "env");

    // Start any declared sidecars and inject their addresses into the main container's env.
    // Any sidecar failure (missing creds, start error, readiness timeout) is fatal: tear
    // down already-started sidecars and bail so the misconfiguration surfaces equally
    // on dev machines and in CI.
    let mut started_sidecars: Vec<SidecarHandle> = vec![];
    if !workspace_meta.sidecars.is_empty() {
        for spec in &workspace_meta.sidecars {
            match start_sidecar(repo, spec, runtime) {
                Ok(handle) => {
                    for (k, v) in &spec.inject {
                        env_vars.push((
                            k.clone(),
                            v.replace(SIDECAR_IP_PLACEHOLDER, &handle.step_address),
                        ));
                    }
                    started_sidecars.push(handle);
                }
                Err(e) => {
                    for handle in &started_sidecars {
                        let _ = runtime.stop_sidecar(handle);
                    }
                    anyhow::bail!(
                        "[{}] sidecar '{}' failed to start: {e}",
                        workspace_meta.label,
                        spec.name
                    );
                }
            }
        }
    }
    let sidecars_for_cleanup = started_sidecars.clone();
    let _sidecar_cleanup = defer::defer(move || {
        for handle in &sidecars_for_cleanup {
            let _ = runtime.stop_sidecar(handle);
        }
    });

    // Create an ephemeral scratch artifact seeded with the worktree contents. The
    // runtime owns its naming and ownership; we just hold the opaque name.
    let worktree_tar = crate::archive::tree_to_tar(repo, worktree_oid)?;
    let snapshot_vol = runtime.create_scratch_artifact(&worktree_tar)?;

    let snapshot_vol_clone = snapshot_vol.clone();
    let _cleanup = defer::defer(move || {
        let _ = runtime.remove_artifact(&snapshot_vol_clone, false);
    });

    let workdir = "/worktree";
    let mut mounts: Vec<Mount> = vec![];
    mounts.push(Mount {
        artifact: snapshot_vol.clone(),
        path: workdir.to_string(),
        read_only: false,
    });

    if workspace_meta.output != OutputMode::None {
        runtime.recreate_artifact(&out_vol)?;
        mounts.push(Mount {
            artifact: out_vol.clone(),
            path: "/out".to_string(),
            read_only: false,
        });
    }

    for (dep_vol, mount, ro) in &dep_volumes {
        mounts.push(Mount {
            artifact: dep_vol.clone(),
            path: mount.clone(),
            read_only: *ro,
        });
    }

    if let Some(cache_vol) = &cache_volume {
        mounts.push(Mount {
            artifact: cache_vol.clone(),
            path: "/opt/cache".to_string(),
            read_only: false,
        });
    }

    let output = runtime.run(RunArgs {
        env: image_name,
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            workspace_meta.cmd.clone(),
        ],
        mounts,
        env_vars,
        network: workspace_meta.network.clone(),
        sidecars: started_sidecars,
        working_dir: Some(workdir.to_string()),
    })?;

    let success = output.exit_code == 0;
    job_cache::write_result(&hash, success, &output.stdout, &output.stderr);

    if workspace_meta.output == OutputMode::Workdir && extract_to_workdir {
        runtime.extract_artifact(&out_vol, Path::new("."))?;
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
