use std::collections::HashMap;

use anyhow::Context;

use crate::naming;
use josh_compose_backend::{BuildContext, EnvRecipe, Runtime};
use josh_compose_graph::Graph;
use josh_core::cache;
use josh_core::memodb;

/// Ensure the environment for the given image exists, building it if needed.
/// Returns the environment key (image name).
///
/// Base images are ensured first and passed as build args. Artifact-producing
/// input jobs must already have completed; their outputs become named contexts.
pub fn ensure_image(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    graph: &Graph,
    image_oid: gix_hash::ObjectId,
    failed: &HashMap<gix_hash::ObjectId, String>,
    runtime: &dyn Runtime,
) -> anyhow::Result<String> {
    let image_name = naming::env(image_oid);
    let node = graph
        .image(image_oid)
        .with_context(|| format!("image {image_oid} is not in the build graph"))?;

    if runtime.env_exists(&image_name)? {
        eprintln!("[image:{}] Already built", node.label);
        return Ok(image_name);
    }

    eprintln!("[image:{}] Building...", node.label);

    let mut build_args: Vec<(String, String)> = vec![];
    for (base_name, base_oid) in &node.bases {
        let base_env = ensure_image(transaction, odb, graph, *base_oid, failed, runtime)?;
        build_args.push((base_name.clone(), base_env));
    }
    build_args.extend(node.args.iter().cloned());

    let mut build_contexts = Vec::with_capacity(node.inputs.len());
    for (name, input_oid) in &node.inputs {
        if let Some(error) = failed.get(input_oid) {
            anyhow::bail!("image {} input {name:?} failed: {error}", node.label);
        }
        let artifact = naming::output(*input_oid);
        if !runtime.artifact_exists(&artifact)? {
            anyhow::bail!(
                "image {} input {name:?} has no output artifact from job {input_oid}",
                node.label
            );
        }
        build_contexts.push(BuildContext {
            name: name.clone(),
            artifact,
        });
    }

    let context_oid = node
        .context
        .context("workspace image tree missing 'context' subtree")?;
    let context = crate::archive::tree_to_tar(transaction, odb, context_oid)?;

    runtime.prepare_env(
        &image_name,
        EnvRecipe {
            context,
            build_args,
            build_contexts,
        },
    )?;

    eprintln!("[image:{}] Built successfully", node.label);
    Ok(image_name)
}
