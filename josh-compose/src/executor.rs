//! The sequential executor: runs a loaded build [`Graph`] against a runtime
//! backend one job at a time, in dependency order. The [`Executor`] strategy
//! contract itself lives in `josh-compose-backend`.

use std::collections::HashMap;
use std::path::Path;

use josh_compose_backend::{
    ExecOpts, Executor, Mount, RunArgs, Runtime, SidecarArgs, SidecarHandle,
};
use josh_compose_graph::{Graph, Job, OutputMode, SidecarSpec};
use josh_core::cache;
use josh_core::memodb;

use crate::image;
use crate::job_cache;
use crate::naming;

const SIDECAR_IP_PLACEHOLDER: &str = "{SIDECAR_IP}";

/// Depth-first, one-job-at-a-time executor — the default strategy.
///
/// Jobs run in the graph's dependency order. A job whose dependencies failed is
/// skipped, but independent branches still run; the run fails if the root job
/// (or any job it transitively depends on) failed.
pub struct SequentialExecutor;

impl Executor for SequentialExecutor {
    fn execute(
        &self,
        transaction: &cache::Transaction,
        graph: &Graph,
        runtime: &dyn Runtime,
        opts: &ExecOpts,
    ) -> anyhow::Result<()> {
        let odb = transaction.odb();
        let mut failed: HashMap<gix_hash::ObjectId, String> = HashMap::new();
        for job in graph.jobs() {
            if let Err(e) = run_job(transaction, odb, graph, job, &failed, runtime, opts) {
                failed.insert(job.ws_tree, e.to_string());
            }
        }

        let root = graph.root();
        if let Some(e) = failed.get(&root.ws_tree) {
            anyhow::bail!("{e}");
        }
        Ok(())
    }
}

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
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    graph: &Graph,
    spec: &SidecarSpec,
    runtime: &dyn Runtime,
) -> anyhow::Result<SidecarHandle> {
    let env_key = image::ensure_image(transaction, odb, graph, spec.image, runtime)?;

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

/// Run a single job whose dependencies have already been attempted. Failures in
/// sibling dependencies are collected so all are reported before the job bails.
///
/// Sidecars declared in the job's metadata are started before the main step and
/// torn down when this function returns (including on error paths), via
/// [`defer::defer`]. A scratch artifact seeded from the worktree is created and
/// cleaned up the same way.
fn run_job(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    graph: &Graph,
    job: &Job,
    failed: &HashMap<gix_hash::ObjectId, String>,
    runtime: &dyn Runtime,
    opts: &ExecOpts,
) -> anyhow::Result<()> {
    let workspace_meta = &job.meta;

    // Cache check: skip only if a previous successful run is recorded AND its
    // output volume is still present (when one is expected). A stale marker
    // without its volume — reachable when an R2 volume pull fails after the
    // marker pull succeeded — self-heals by re-running rather than failing
    // downstream dep-mounts.
    let hash = job.ws_tree.to_string();
    let out_vol = naming::output(job.ws_tree);
    if job_cache::is_cached_success(&hash)
        && (workspace_meta.output == OutputMode::None || runtime.artifact_exists(&out_vol)?)
    {
        eprintln!(
            "[{}] Using cached output ({})",
            workspace_meta.label, job.ws_tree
        );
        return Ok(());
    }

    eprintln!("[{}] Running ({})", workspace_meta.label, job.ws_tree);

    // Dependencies completed earlier in graph order. Collect the output volumes
    // to mount; a failed dependency is fatal to this job but not to siblings.
    let mut dep_volumes: Vec<(String, String, bool)> = vec![];
    let mut dep_errors: Vec<String> = vec![];
    for (dep_name, dep_tree) in &job.inputs {
        if let Some(e) = failed.get(dep_tree) {
            dep_errors.push(format!("dependency {dep_name} failed: {e}"));
            continue;
        }
        let dep_meta = &graph
            .job(*dep_tree)
            .expect("job inputs reference jobs in the graph")
            .meta;
        if dep_meta.output == OutputMode::None {
            continue;
        }
        let dep_out_vol = naming::output(*dep_tree);
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
    let image_name = image::ensure_image(transaction, odb, graph, image_oid, runtime)?;

    let mut cache_volume: Option<String> = None;
    if let Some(cache_name) = &workspace_meta.cache {
        let vol_name = naming::cache(cache_name);
        runtime.ensure_artifact(&vol_name)?;
        cache_volume = Some(vol_name);
    }

    let mut env_vars = job.env.clone();

    // Start any declared sidecars and inject their addresses into the main container's env.
    // Any sidecar failure (missing creds, start error, readiness timeout) is fatal: tear
    // down already-started sidecars and bail so the misconfiguration surfaces equally
    // on dev machines and in CI.
    let mut started_sidecars: Vec<SidecarHandle> = vec![];
    if !workspace_meta.sidecars.is_empty() {
        for spec in &workspace_meta.sidecars {
            match start_sidecar(transaction, odb, graph, spec, runtime) {
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
    let worktree_tar = crate::archive::tree_to_tar(transaction, odb, worktree_oid)?;
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

    if workspace_meta.output == OutputMode::Workdir && opts.extract_to_workdir {
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
