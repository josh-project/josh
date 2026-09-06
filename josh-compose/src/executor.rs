//! The sequential executor: runs a loaded build [`Graph`] against a runtime
//! backend one job at a time, in dependency order. The [`Executor`] strategy
//! contract itself lives in `josh-compose-backend`.

use std::collections::{HashMap, HashSet};
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
use crate::plan;

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
        let scheduled = scheduled_jobs(transaction, graph, runtime)?;
        let mut failed: HashMap<gix_hash::ObjectId, String> = HashMap::new();
        let mut pending = job_cache::PendingResults::default();
        for job in graph.jobs() {
            if !scheduled.contains(&job.ws_tree) {
                continue;
            }
            if let Err(e) = run_job(
                transaction,
                odb,
                graph,
                job,
                &failed,
                &mut pending,
                runtime,
                opts,
            ) {
                failed.insert(job.ws_tree, e.to_string());
            }
        }

        let run_error = failed.get(&graph.root().ws_tree).cloned();
        if let Err(commit_error) = job_cache::commit_results_persistent(transaction, pending) {
            return match run_error {
                None => Err(commit_error),
                Some(run_error) => Err(commit_error.context(format!(
                    "workspace run also failed before results were committed: {run_error}"
                ))),
            };
        }
        if let Some(run_error) = run_error {
            anyhow::bail!("{run_error}");
        }
        Ok(())
    }
}

/// Select jobs reachable through cache-missing jobs and images. A cached image
/// cuts off its artifact-producing input jobs just as a cached job cuts off its
/// ordinary inputs.
fn scheduled_jobs(
    transaction: &cache::Transaction,
    graph: &Graph,
    runtime: &dyn Runtime,
) -> anyhow::Result<HashSet<gix_hash::ObjectId>> {
    let mut scheduled = HashSet::new();
    let mut visited_images = HashSet::new();
    let mut stack = vec![graph.root().ws_tree];

    while let Some(ws_tree) = stack.pop() {
        if !scheduled.insert(ws_tree) {
            continue;
        }
        let job = graph
            .job(ws_tree)
            .expect("scheduled jobs are present in the graph");
        if plan::workspace_is_skippable(transaction, job, runtime)? {
            continue;
        }

        stack.extend(job.inputs.iter().map(|(_, input_oid)| *input_oid));
        if let Some(image_oid) = job.meta.image {
            schedule_image_inputs(graph, image_oid, runtime, &mut visited_images, &mut stack)?;
        }
        for sidecar in &job.meta.sidecars {
            schedule_image_inputs(
                graph,
                sidecar.image,
                runtime,
                &mut visited_images,
                &mut stack,
            )?;
        }
    }

    Ok(scheduled)
}

fn schedule_image_inputs(
    graph: &Graph,
    image_oid: gix_hash::ObjectId,
    runtime: &dyn Runtime,
    visited: &mut HashSet<gix_hash::ObjectId>,
    jobs: &mut Vec<gix_hash::ObjectId>,
) -> anyhow::Result<()> {
    if !visited.insert(image_oid) || runtime.env_exists(&naming::env(image_oid))? {
        return Ok(());
    }

    let image = graph
        .image(image_oid)
        .expect("scheduled images are present in the graph");
    for (_, base_oid) in &image.bases {
        schedule_image_inputs(graph, *base_oid, runtime, visited, jobs)?;
    }
    jobs.extend(image.inputs.iter().map(|(_, input_oid)| *input_oid));
    Ok(())
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
    failed: &HashMap<gix_hash::ObjectId, String>,
    runtime: &dyn Runtime,
) -> anyhow::Result<SidecarHandle> {
    let env_key = image::ensure_image(transaction, odb, graph, spec.image, failed, runtime)?;

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
    pending: &mut job_cache::PendingResults,
    runtime: &dyn Runtime,
    opts: &ExecOpts,
) -> anyhow::Result<()> {
    let workspace_meta = &job.meta;

    // Cache check: skip only if a previous successful run is recorded AND its
    // output volume is still present (when one is expected). A stale marker
    // without its volume — reachable when an R2 volume pull fails after the
    // marker pull succeeded — self-heals by re-running rather than failing
    // downstream dep-mounts.
    let out_vol = naming::output(job.ws_tree);
    if job_cache::is_cached_success(transaction, job.ws_tree)?
        && (workspace_meta.output == OutputMode::None || runtime.artifact_exists(&out_vol)?)
    {
        if workspace_meta.output != OutputMode::None {
            pending.touch(job.ws_tree);
        }
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
        pending.record(job.ws_tree, true, Vec::new(), Vec::new());
        eprintln!("[{}] Done (orchestrator)", workspace_meta.label);
        return Ok(());
    };
    let Some(worktree_oid) = workspace_meta.worktree else {
        pending.record(job.ws_tree, true, Vec::new(), Vec::new());
        eprintln!("[{}] Done (no worktree)", workspace_meta.label);
        return Ok(());
    };

    // Resolve the environment (cache-or-build).
    let image_name = image::ensure_image(transaction, odb, graph, image_oid, failed, runtime)?;

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
            match start_sidecar(transaction, odb, graph, spec, failed, runtime) {
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

    let exit_code = output.exit_code;
    let success = exit_code == 0;
    pending.record(job.ws_tree, success, output.stdout, output.stderr);

    if workspace_meta.output == OutputMode::Workdir && opts.extract_to_workdir {
        runtime.extract_artifact(&out_vol, Path::new("."))?;
    }

    if !success {
        anyhow::bail!(
            "[{}] FAILED with exit code {}",
            workspace_meta.label,
            exit_code
        );
    }

    eprintln!("[{}] SUCCESS", workspace_meta.label);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use josh_compose_backend::{
        ArtifactBackend, EnvRecipe, EnvironmentBackend, ExecutionBackend, RunOutput, StorageStatus,
    };
    use josh_core::filter::tree;
    use parking_lot::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeRuntime {
        artifacts: Mutex<HashSet<String>>,
        envs: Mutex<HashSet<String>>,
        builds: Mutex<Vec<(String, EnvRecipe)>>,
        runs: Mutex<usize>,
    }

    impl EnvironmentBackend for FakeRuntime {
        fn env_exists(&self, key: &str) -> anyhow::Result<bool> {
            Ok(self.envs.lock().contains(key))
        }

        fn prepare_env(&self, key: &str, recipe: EnvRecipe) -> anyhow::Result<()> {
            self.envs.lock().insert(key.to_string());
            self.builds.lock().push((key.to_string(), recipe));
            Ok(())
        }

        fn list_envs(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(self
                .envs
                .lock()
                .iter()
                .filter(|name| name.starts_with(prefix))
                .cloned()
                .collect())
        }

        fn remove_env(&self, key: &str) -> anyhow::Result<()> {
            self.envs.lock().remove(key);
            Ok(())
        }
    }

    impl ArtifactBackend for FakeRuntime {
        fn artifact_exists(&self, name: &str) -> anyhow::Result<bool> {
            Ok(self.artifacts.lock().contains(name))
        }

        fn create_artifact(&self, name: &str) -> anyhow::Result<()> {
            self.artifacts.lock().insert(name.to_string());
            Ok(())
        }

        fn import_artifact(&self, name: &str, _tar: &[u8]) -> anyhow::Result<()> {
            self.artifacts.lock().insert(name.to_string());
            Ok(())
        }

        fn export_artifact(&self, _name: &str) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn extract_artifact(&self, _name: &str, _dest: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }

        fn remove_artifact(&self, name: &str, _force: bool) -> anyhow::Result<()> {
            self.artifacts.lock().remove(name);
            Ok(())
        }

        fn list_artifacts(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(self
                .artifacts
                .lock()
                .iter()
                .filter(|name| name.starts_with(prefix))
                .cloned()
                .collect())
        }

        fn storage_status(&self) -> anyhow::Result<Option<StorageStatus>> {
            Ok(None)
        }

        fn create_scratch_artifact(&self, _tar: &[u8]) -> anyhow::Result<String> {
            let name = format!("scratch-{}", self.artifacts.lock().len());
            self.artifacts.lock().insert(name.clone());
            Ok(name)
        }
    }

    impl ExecutionBackend for FakeRuntime {
        fn run(&self, args: RunArgs) -> anyhow::Result<RunOutput> {
            *self.runs.lock() += 1;
            for mount in args.mounts {
                if mount.path == "/out" {
                    self.artifacts.lock().insert(mount.artifact);
                }
            }
            Ok(RunOutput {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }

        fn start_sidecar(&self, _args: SidecarArgs) -> anyhow::Result<SidecarHandle> {
            anyhow::bail!("sidecars are not used by these tests")
        }

        fn stop_sidecar(&self, _handle: &SidecarHandle) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn insert_blob(
        odb: &memodb::Odb,
        root: gix_hash::ObjectId,
        path: &str,
        contents: &[u8],
    ) -> gix_hash::ObjectId {
        let blob = josh_core::objects::write_blob(odb, contents).unwrap();
        tree::insert_oid(odb, root, Path::new(path), blob, 0o100644).unwrap()
    }

    fn image(odb: &memodb::Odb, dockerfile: &[u8]) -> gix_hash::ObjectId {
        let context = insert_blob(odb, tree::empty_id(), "Dockerfile", dockerfile);
        tree::insert_oid(
            odb,
            tree::empty_id(),
            Path::new("context"),
            context,
            0o040000,
        )
        .unwrap()
    }

    fn job(odb: &memodb::Odb, label: &[u8], image: gix_hash::ObjectId) -> gix_hash::ObjectId {
        let worktree = insert_blob(odb, tree::empty_id(), "run.sh", b"true");
        let root = insert_blob(odb, tree::empty_id(), "label", label);
        let root = tree::insert_oid(odb, root, Path::new("image"), image, 0o160000).unwrap();
        tree::insert_oid(odb, root, Path::new("worktree"), worktree, 0o040000).unwrap()
    }

    fn graph_with_image_input(
        transaction: &cache::Transaction,
    ) -> (
        Graph,
        gix_hash::ObjectId,
        gix_hash::ObjectId,
        gix_hash::ObjectId,
    ) {
        let odb = transaction.odb();
        let producer_image = image(odb, b"FROM scratch\n");
        let producer = job(odb, b"producer", producer_image);

        let input_tree = tree::insert_oid(
            odb,
            tree::empty_id(),
            Path::new("binary"),
            producer,
            0o160000,
        )
        .unwrap();
        let consumer_image = tree::insert_oid(
            odb,
            image(odb, b"FROM scratch\nCOPY --from=binary /app /app\n"),
            Path::new("inputs"),
            input_tree,
            0o040000,
        )
        .unwrap();
        let root = job(odb, b"consumer", consumer_image);
        (
            josh_compose_graph::load_graph(transaction, odb, root).unwrap(),
            producer,
            producer_image,
            consumer_image,
        )
    }

    #[test]
    fn builds_image_from_normal_job_output() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context =
            cache::TransactionContext::new(dir.path(), Arc::new(cache::CacheStack::new()));
        let transaction = context.open().unwrap();
        let (graph, producer, _, consumer_image) = graph_with_image_input(&transaction);
        let runtime = FakeRuntime::default();

        SequentialExecutor
            .execute(
                &transaction,
                &graph,
                &runtime,
                &ExecOpts {
                    extract_to_workdir: false,
                },
            )
            .unwrap();

        assert_eq!(*runtime.runs.lock(), 2);
        let builds = runtime.builds.lock();
        let (_, recipe) = builds
            .iter()
            .find(|(name, _)| name == &naming::env(consumer_image))
            .unwrap();
        assert_eq!(recipe.build_contexts.len(), 1);
        assert_eq!(recipe.build_contexts[0].name, "binary");
        assert_eq!(recipe.build_contexts[0].artifact, naming::output(producer));
    }

    #[test]
    fn cached_image_prunes_its_producer_job() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context =
            cache::TransactionContext::new(dir.path(), Arc::new(cache::CacheStack::new()));
        let transaction = context.open().unwrap();
        let (graph, _, _, consumer_image) = graph_with_image_input(&transaction);
        let runtime = FakeRuntime::default();
        runtime.envs.lock().insert(naming::env(consumer_image));

        SequentialExecutor
            .execute(
                &transaction,
                &graph,
                &runtime,
                &ExecOpts {
                    extract_to_workdir: false,
                },
            )
            .unwrap();

        assert_eq!(*runtime.runs.lock(), 1);
        assert!(runtime.builds.lock().is_empty());
    }
}
