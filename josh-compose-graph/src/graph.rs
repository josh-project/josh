//! Build-graph loading: resolve the full workspace, image, and artifact dependency
//! closure from git trees into an in-memory [`Graph`], before execution starts.
//!
//! Every edge in the graph is a gitlink — a commit-mode tree entry holding the
//! target OID (`inputs/<name>` for workspace and image artifact dependencies,
//! `bases/<name>` for image bases) — so the closure is static and fully determined
//! by the root workspace tree. Executors consume the loaded graph and never walk
//! git trees themselves; only heavy byte payloads (build contexts and worktree
//! tars) stay lazy, addressed by tree OID.

use std::collections::HashMap;
use std::fmt;

use josh_core::cache;
use josh_core::filter::tree;
use josh_core::memodb;

use crate::meta::{self, WorkspaceMeta};

/// A fully-resolved build graph: every workspace a run would touch and every
/// image it would build.
pub struct Graph {
    /// Workspaces in dependency order (a workspace always appears after its
    /// inputs), deduplicated. The last job is the run's root.
    pub(super) jobs: Vec<Job>,
    /// Images in bases-first order (a base image always appears before any
    /// image that uses it), deduplicated.
    pub(super) images: Vec<ImageNode>,
    pub(super) job_index: HashMap<gix_hash::ObjectId, usize>,
    pub(super) image_index: HashMap<gix_hash::ObjectId, usize>,
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
    /// Human-readable label used in console and graph output.
    pub label: String,
    /// Base images as (build-arg name, base image tree OID).
    pub bases: Vec<(String, gix_hash::ObjectId)>,
    /// Artifact-producing jobs as (named build-context name, workspace tree OID).
    pub inputs: Vec<(String, gix_hash::ObjectId)>,
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
    let mut stack: Vec<NodeId> = vec![];
    load_job(transaction, odb, ws_tree, &mut graph, &mut stack)?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeId {
    Job(gix_hash::ObjectId),
    Image(gix_hash::ObjectId),
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Job(oid) => write!(f, "job {oid}"),
            Self::Image(oid) => write!(f, "image {oid}"),
        }
    }
}

fn enter_node(stack: &mut Vec<NodeId>, node: NodeId) -> anyhow::Result<()> {
    if let Some(start) = stack.iter().position(|candidate| *candidate == node) {
        let mut path = stack[start..]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        path.push(node.to_string());
        anyhow::bail!("dependency cycle detected: {}", path.join(" -> "));
    }
    stack.push(node);
    Ok(())
}

fn load_job(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    ws_tree: gix_hash::ObjectId,
    graph: &mut Graph,
    stack: &mut Vec<NodeId>,
) -> anyhow::Result<()> {
    if graph.job_index.contains_key(&ws_tree) {
        return Ok(());
    }
    enter_node(stack, NodeId::Job(ws_tree))?;

    let meta = meta::read_meta(transaction, odb, ws_tree)?;
    let mut inputs = vec![];
    for (dep_name, dep_tree) in meta::read_gitlink_entries(transaction, odb, ws_tree, "inputs")? {
        load_job(transaction, odb, dep_tree, graph, stack)?;
        inputs.push((dep_name, dep_tree));
    }

    if let Some(image_oid) = meta.image {
        load_image(transaction, odb, image_oid, graph, stack)?;
    }
    for spec in &meta.sidecars {
        load_image(transaction, odb, spec.image, graph, stack)?;
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
    assert_eq!(stack.pop(), Some(NodeId::Job(ws_tree)));
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
    stack: &mut Vec<NodeId>,
) -> anyhow::Result<()> {
    if graph.image_index.contains_key(&image_oid) {
        return Ok(());
    }
    enter_node(stack, NodeId::Image(image_oid))?;
    let label = meta::read_blob(transaction, odb, image_oid, "label")
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| image_oid.to_string());

    let mut bases = vec![];
    for (base_name, base_oid) in meta::read_gitlink_entries(transaction, odb, image_oid, "bases")? {
        load_image(transaction, odb, base_oid, graph, stack)?;
        bases.push((base_name, base_oid));
    }

    let mut inputs = vec![];
    for (input_name, input_oid) in
        meta::read_gitlink_entries(transaction, odb, image_oid, "inputs")?
    {
        validate_build_context_name(&input_name)?;
        load_job(transaction, odb, input_oid, graph, stack)?;
        let input = graph
            .job(input_oid)
            .expect("loaded image inputs are present in the graph");
        if input.meta.output == crate::OutputMode::None {
            anyhow::bail!(
                "image {image_oid} input {input_name:?} references job {input_oid} with output=none"
            );
        }
        inputs.push((input_name, input_oid));
    }

    let args = meta::read_blob_entries(transaction, odb, image_oid, "args");
    let context = tree::read_tree(transaction, odb, image_oid)?
        .entry(b"context")
        .map(|e| e.oid.to_owned());

    push_image(
        graph,
        ImageNode {
            oid: image_oid,
            label,
            bases,
            inputs,
            args,
            context,
        },
    );
    assert_eq!(stack.pop(), Some(NodeId::Image(image_oid)));
    Ok(())
}

fn validate_build_context_name(name: &str) -> anyhow::Result<()> {
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'));
    if !valid {
        anyhow::bail!("invalid image input name {name:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(digit: u8) -> gix_hash::ObjectId {
        gix_hash::ObjectId::from_hex(&[digit; 40]).unwrap()
    }

    fn insert_blob(
        odb: &memodb::Odb,
        root: gix_hash::ObjectId,
        path: &str,
        contents: &[u8],
    ) -> gix_hash::ObjectId {
        let blob = josh_core::objects::write_blob(odb, contents).unwrap();
        tree::insert_oid(odb, root, std::path::Path::new(path), blob, 0o100644).unwrap()
    }

    #[test]

    fn loads_image_artifact_inputs_before_the_consuming_job() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let odb = transaction.odb();

        let producer = insert_blob(odb, tree::empty_id(), "label", b"producer");
        let image_inputs = tree::insert_oid(
            odb,
            tree::empty_id(),
            std::path::Path::new("artifact"),
            producer,
            0o160000,
        )
        .unwrap();
        let image = tree::insert_oid(
            odb,
            insert_blob(odb, tree::empty_id(), "label", b"artifact image"),
            std::path::Path::new("inputs"),
            image_inputs,
            0o040000,
        )
        .unwrap();
        let root = tree::insert_oid(
            odb,
            insert_blob(odb, tree::empty_id(), "label", b"root"),
            std::path::Path::new("image"),
            image,
            0o160000,
        )
        .unwrap();

        let graph = load_graph(&transaction, odb, root).unwrap();
        assert_eq!(
            graph
                .jobs()
                .iter()
                .map(|job| job.ws_tree)
                .collect::<Vec<_>>(),
            vec![producer, root]
        );
        assert_eq!(
            graph.image(image).unwrap().inputs,
            vec![("artifact".to_string(), producer)]
        );
        assert_eq!(graph.image(image).unwrap().label, "artifact image");
    }

    #[test]
    fn rejects_image_inputs_without_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let odb = transaction.odb();

        let producer = insert_blob(odb, tree::empty_id(), "output", b"none");
        let image_inputs = tree::insert_oid(
            odb,
            tree::empty_id(),
            std::path::Path::new("artifact"),
            producer,
            0o160000,
        )
        .unwrap();
        let image = tree::insert_oid(
            odb,
            tree::empty_id(),
            std::path::Path::new("inputs"),
            image_inputs,
            0o040000,
        )
        .unwrap();
        let root = tree::insert_oid(
            odb,
            tree::empty_id(),
            std::path::Path::new("image"),
            image,
            0o160000,
        )
        .unwrap();

        let error = match load_graph(&transaction, odb, root) {
            Ok(_) => panic!("image input without an artifact was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            format!("image {image} input \"artifact\" references job {producer} with output=none")
        );
    }

    #[test]
    fn mixed_cycles_report_the_dependency_path() {
        let job = NodeId::Job(oid(b'1'));
        let image = NodeId::Image(oid(b'2'));
        let mut stack = vec![job, image];

        assert_eq!(
            enter_node(&mut stack, job).unwrap_err().to_string(),
            format!("dependency cycle detected: {job} -> {image} -> {job}")
        );
    }
}
