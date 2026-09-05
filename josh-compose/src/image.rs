use anyhow::Context;

use crate::naming;
use josh_compose_backend::{EnvRecipe, EnvironmentBackend};
use josh_compose_graph::Graph;
use josh_core::cache;
use josh_core::memodb;

/// Ensure the environment for the given image exists, building it if needed.
/// Returns the environment key (image name).
///
/// Base images are ensured first, recursively, and passed to the build as args
/// so the Containerfile can reference them (e.g. `ARG my_base; FROM $my_base`).
pub fn ensure_image(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    graph: &Graph,
    image_oid: gix_hash::ObjectId,
    runtime: &dyn EnvironmentBackend,
) -> anyhow::Result<String> {
    let image_name = naming::env(image_oid);

    if runtime.env_exists(&image_name)? {
        eprintln!("[image:{image_name}] Already built");
        return Ok(image_name);
    }

    eprintln!("[image:{image_name}] Building...");

    let node = graph
        .image(image_oid)
        .with_context(|| format!("image {image_oid} is not in the build graph"))?;

    let mut build_args: Vec<(String, String)> = vec![];
    for (base_name, base_oid) in &node.bases {
        let base_env = ensure_image(transaction, odb, graph, *base_oid, runtime)?;
        build_args.push((base_name.clone(), base_env));
    }
    build_args.extend(node.args.iter().cloned());

    let context_oid = node
        .context
        .context("workspace image tree missing 'context' subtree")?;
    let context = crate::archive::tree_to_tar(transaction, odb, context_oid)?;

    runtime.prepare_env(
        &image_name,
        EnvRecipe {
            context,
            build_args,
        },
    )?;

    eprintln!("[image:{image_name}] Built successfully");
    Ok(image_name)
}
