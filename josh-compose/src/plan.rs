//! Plan listings computed over the loaded build [`Graph`].
//! The graph loader ([`josh_compose_graph::load_graph`]) resolves the full
//! dependency closure; the functions here only filter it by cache state. A
//! workspace is skipped when its run is already cached successful AND its output
//! volume is still present — mirroring the executor's cache check in
//! `executor::run_job`, so a stale marker without its volume self-heals by being
//! planned (and later run) again.

use std::collections::HashSet;

use josh_compose_backend::{ArtifactBackend, Runtime};
use josh_compose_graph::{Graph, Job, OutputMode, load_graph};
use josh_core::cache;
use josh_core::memodb;

use crate::job_cache;
use crate::naming;

/// Collect every image build-tree OID a run would touch: the images of all
/// runnable jobs (including sidecar images) and their transitive bases,
/// ordered bases-first and deduplicated. This is the order in which images
/// would need to be pulled/built for a run to succeed.
///
/// When `ignore_cache` is false (the default), workspaces whose run is already
/// cached successful are pruned — along with their dependency subtrees — so
/// only images a run would actually build are reported. When `ignore_cache` is
/// true, every image a fresh-cache run would build is reported.
pub fn collect_image_oids(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
    ignore_cache: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let graph = load_graph(transaction, odb, ws_tree)?;
    let runnable = runnable_jobs(transaction, &graph, ignore_cache, runtime, true)?;

    let mut wanted: HashSet<gix_hash::ObjectId> = HashSet::new();
    for job in graph.jobs() {
        if !runnable.contains(&job.ws_tree) {
            continue;
        }
        if let Some(image_oid) = job.meta.image {
            collect_with_bases(&graph, image_oid, &mut wanted);
        }
        for spec in &job.meta.sidecars {
            collect_with_bases(&graph, spec.image, &mut wanted);
        }
    }

    // The graph's image list is already bases-first and deduplicated; filtering
    // it preserves both properties.
    Ok(graph
        .images()
        .iter()
        .filter(|image| wanted.contains(&image.oid))
        .map(|image| image.oid)
        .collect())
}

/// Collect images directly used to execute one workspace, including sidecars and
/// transitive base images but excluding dependency workspaces.
pub(crate) fn collect_workspace_image_oids(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let graph = load_graph(transaction, odb, ws_tree)?;
    let job = graph.root();
    let mut wanted = HashSet::new();

    if let Some(image_oid) = job.meta.image {
        collect_with_bases(&graph, image_oid, &mut wanted);
    }
    for spec in &job.meta.sidecars {
        collect_with_bases(&graph, spec.image, &mut wanted);
    }

    Ok(graph
        .images()
        .iter()
        .filter(|image| wanted.contains(&image.oid))
        .map(|image| image.oid)
        .collect())
}

/// Collect the job hash (ws_tree OID) of every workspace a run would touch, in
/// dependency order (dependencies first) — including orchestrator workspaces
/// that don't build an image, since those still write a `job_cache` entry.
///
/// Cache semantics mirror `collect_image_oids`: when `ignore_cache` is false,
/// cached-successful workspaces are pruned with their dependency subtrees; when
/// true, every job a fresh-cache run would touch is reported.
pub fn collect_job_hashes(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
    ignore_cache: bool,
    runtime: &dyn Runtime,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let graph = load_graph(transaction, odb, ws_tree)?;
    let runnable = runnable_jobs(transaction, &graph, ignore_cache, runtime, false)?;

    Ok(graph
        .jobs()
        .iter()
        .filter(|job| runnable.contains(&job.ws_tree))
        .map(|job| job.ws_tree)
        .collect())
}

/// Compute the set of jobs a run would execute. Cached jobs prune ordinary
/// inputs; cached images prune artifact-producing image inputs.
fn runnable_jobs(
    transaction: &cache::Transaction,
    graph: &Graph,
    ignore_cache: bool,
    runtime: &dyn Runtime,
    log_skips: bool,
) -> anyhow::Result<HashSet<gix_hash::ObjectId>> {
    let mut visited_jobs = HashSet::new();
    let mut visited_images = HashSet::new();
    let mut runnable = HashSet::new();
    let mut stack = vec![graph.root().ws_tree];
    while let Some(ws_tree) = stack.pop() {
        if !visited_jobs.insert(ws_tree) {
            continue;
        }
        let job = graph
            .job(ws_tree)
            .expect("job dependencies are present in the graph");
        if !ignore_cache && workspace_is_skippable(transaction, job, runtime)? {
            if log_skips {
                eprintln!("[{}] Using cached output ({})", job.meta.label, ws_tree);
            }
            continue;
        }
        runnable.insert(ws_tree);
        stack.extend(job.inputs.iter().map(|(_, dep_tree)| *dep_tree));
        if let Some(image_oid) = job.meta.image {
            collect_image_input_jobs(
                graph,
                image_oid,
                ignore_cache,
                runtime,
                &mut visited_images,
                &mut stack,
            )?;
        }
        for sidecar in &job.meta.sidecars {
            collect_image_input_jobs(
                graph,
                sidecar.image,
                ignore_cache,
                runtime,
                &mut visited_images,
                &mut stack,
            )?;
        }
    }
    Ok(runnable)
}

fn collect_image_input_jobs(
    graph: &Graph,
    image_oid: gix_hash::ObjectId,
    ignore_cache: bool,
    runtime: &dyn Runtime,
    visited: &mut HashSet<gix_hash::ObjectId>,
    jobs: &mut Vec<gix_hash::ObjectId>,
) -> anyhow::Result<()> {
    if !visited.insert(image_oid)
        || (!ignore_cache && runtime.env_exists(&naming::env(image_oid))?)
    {
        return Ok(());
    }
    let image = graph
        .image(image_oid)
        .expect("image dependencies are present in the graph");
    for (_, base_oid) in &image.bases {
        collect_image_input_jobs(graph, *base_oid, ignore_cache, runtime, visited, jobs)?;
    }
    jobs.extend(image.inputs.iter().map(|(_, input_oid)| *input_oid));
    Ok(())
}

/// Mirror the executor's cache check: a job is skippable when a previous
/// successful run is recorded AND its output volume still exists (when one is
/// expected).
pub(crate) fn workspace_is_skippable<R: ArtifactBackend + ?Sized>(
    transaction: &cache::Transaction,
    job: &Job,
    runtime: &R,
) -> anyhow::Result<bool> {
    if !job_cache::is_cached_success(transaction, job.ws_tree)? {
        return Ok(false);
    }
    if job.meta.output == OutputMode::None {
        return Ok(true);
    }
    runtime.artifact_exists(&naming::output(job.ws_tree))
}

/// Collect an image and all its transitive base images into `wanted`.
fn collect_with_bases(
    graph: &Graph,
    image_oid: gix_hash::ObjectId,
    wanted: &mut HashSet<gix_hash::ObjectId>,
) {
    if wanted.contains(&image_oid) {
        return;
    }
    let node = graph
        .image(image_oid)
        .expect("job images reference images in the graph");
    for (_, base_oid) in &node.bases {
        collect_with_bases(graph, *base_oid, wanted);
    }
    wanted.insert(image_oid);
}
