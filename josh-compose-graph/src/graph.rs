//! Build-graph loading: resolve the full workspace and image dependency closure
//! from git trees into an in-memory [`Graph`], before any execution starts.
//!
//! Every edge in the graph is a gitlink — a commit-mode tree entry holding the
//! target OID (`inputs/<name>` for workspace dependencies, `bases/<name>` for
//! image bases) — so the closure is static and fully determined by the root
//! workspace tree. Executors consume the loaded graph and never walk git trees
//! themselves; only heavy byte payloads (build-context and worktree tars) stay
//! lazy, addressed by tree OID.

use std::collections::HashMap;

use josh_core::cache;
use josh_core::filter::tree;
use josh_core::memodb;

use crate::meta::{self, WorkspaceMeta};

/// A fully-resolved build graph: every workspace a run would touch and every
/// image it would build.
pub struct Graph {
    /// Workspaces in dependency order (a workspace always appears after its
    /// inputs), deduplicated. The last job is the run's root.
    jobs: Vec<Job>,
    /// Images in bases-first order (a base image always appears before any
    /// image that uses it), deduplicated.
    images: Vec<ImageNode>,
    job_index: HashMap<gix_hash::ObjectId, usize>,
    image_index: HashMap<gix_hash::ObjectId, usize>,
}

/// One workspace step in the build graph.
pub struct Job {
    /// Workspace tree OID; also the job's cache key.
    pub ws_tree: gix_hash::ObjectId,
    pub meta: WorkspaceMeta,
    /// Dependencies as (input name, dependency workspace tree OID), in the
    /// order they are declared in the workspace tree.
    pub inputs: Vec<(String, gix_hash::ObjectId)>,
    /// Environment variables from the workspace tree's `env/` subtree.
    pub env: Vec<(String, String)>,
}

/// One image build in the graph. The build context is kept as a tree OID and
/// materialized to a tar lazily by the executor.
pub struct ImageNode {
    pub oid: gix_hash::ObjectId,
    /// Base images as (build-arg name, base image tree OID).
    pub bases: Vec<(String, gix_hash::ObjectId)>,
    /// Additional build args from the image tree's `args/` subtree.
    pub args: Vec<(String, String)>,
    /// OID of the `context` subtree; `None` if the image tree has none. The
    /// executor errors only when the image actually needs building.
    pub context: Option<gix_hash::ObjectId>,
}

impl Graph {
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn images(&self) -> &[ImageNode] {
        &self.images
    }

    pub fn job(&self, ws_tree: gix_hash::ObjectId) -> Option<&Job> {
        self.job_index.get(&ws_tree).map(|&i| &self.jobs[i])
    }

    pub fn image(&self, oid: gix_hash::ObjectId) -> Option<&ImageNode> {
        self.image_index.get(&oid).map(|&i| &self.images[i])
    }

    /// The workspace the run was invoked on: the last job in dependency order.
    pub fn root(&self) -> &Job {
        self.jobs
            .last()
            .expect("graph contains at least the root job")
    }
}

/// Load the build graph reachable from the workspace tree `ws_tree`.
///
/// The whole closure is validated here: malformed references (a non-gitlink
/// entry where an object reference is expected) and dependency cycles are
/// load-time errors, even for workspaces whose run would later be
/// short-circuited by the output cache.
pub fn load_graph(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
) -> anyhow::Result<Graph> {
    let mut graph = Graph {
        jobs: vec![],
        images: vec![],
        job_index: HashMap::new(),
        image_index: HashMap::new(),
    };
    let mut job_stack: Vec<gix_hash::ObjectId> = vec![];
    let mut image_stack: Vec<gix_hash::ObjectId> = vec![];
    load_job(
        transaction,
        odb,
        ws_tree,
        &mut graph,
        &mut job_stack,
        &mut image_stack,
    )?;
    Ok(graph)
}

fn push_job(graph: &mut Graph, job: Job) {
    graph.job_index.insert(job.ws_tree, graph.jobs.len());
    graph.jobs.push(job);
}

fn push_image(graph: &mut Graph, image: ImageNode) {
    graph.image_index.insert(image.oid, graph.images.len());
    graph.images.push(image);
}

fn load_job(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
    graph: &mut Graph,
    job_stack: &mut Vec<gix_hash::ObjectId>,
    image_stack: &mut Vec<gix_hash::ObjectId>,
) -> anyhow::Result<()> {
    if graph.job_index.contains_key(&ws_tree) {
        return Ok(());
    }
    if job_stack.contains(&ws_tree) {
        anyhow::bail!("dependency cycle detected involving workspace {ws_tree}");
    }
    job_stack.push(ws_tree);

    let meta = meta::read_meta(transaction, odb, ws_tree)?;
    let mut inputs = vec![];
    for (dep_name, dep_tree) in meta::read_gitlink_entries(transaction, odb, ws_tree, "inputs")? {
        load_job(transaction, odb, dep_tree, graph, job_stack, image_stack)?;
        inputs.push((dep_name, dep_tree));
    }
    job_stack.pop();

    if let Some(image_oid) = meta.image {
        load_image(transaction, odb, image_oid, graph, image_stack)?;
    }
    for spec in &meta.sidecars {
        load_image(transaction, odb, spec.image, graph, image_stack)?;
    }

    let env = meta::read_blob_entries(transaction, odb, ws_tree, "env");

    push_job(
        graph,
        Job {
            ws_tree,
            meta,
            inputs,
            env,
        },
    );
    Ok(())
}

/// Record an image and all its transitive base images, bases-first.
///
/// The recursive call for each base happens *before* the current image is
/// pushed (post-order traversal), so a base image always appears before any
/// image that uses it as a base — the order images must be pulled/built in.
fn load_image(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    image_oid: gix_hash::ObjectId,
    graph: &mut Graph,
    image_stack: &mut Vec<gix_hash::ObjectId>,
) -> anyhow::Result<()> {
    if graph.image_index.contains_key(&image_oid) {
        return Ok(());
    }
    if image_stack.contains(&image_oid) {
        anyhow::bail!("base image cycle detected involving image {image_oid}");
    }
    image_stack.push(image_oid);

    let mut bases = vec![];
    for (base_name, base_oid) in meta::read_gitlink_entries(transaction, odb, image_oid, "bases")? {
        load_image(transaction, odb, base_oid, graph, image_stack)?;
        bases.push((base_name, base_oid));
    }
    image_stack.pop();

    let args = meta::read_blob_entries(transaction, odb, image_oid, "args");
    let context = tree::read_tree(transaction, odb, image_oid)?
        .entry(b"context")
        .map(|e| e.oid.to_owned());

    push_image(
        graph,
        ImageNode {
            oid: image_oid,
            bases,
            args,
            context,
        },
    );
    Ok(())
}
