use anyhow::Context;
use std::str::FromStr;

use josh_compose_backend::{EnvRecipe, EnvironmentBackend};
use josh_core::cache;
use josh_core::filter::tree;
use josh_core::memodb;

use crate::meta;
use crate::naming;

/// Ensure the environment for the given build tree exists, building it if needed.
/// Returns the environment key (image name).
pub fn ensure_image(
    transaction: &cache::Transaction,
    odb: &memodb::Odb,
    build_tree: gix_hash::ObjectId,
    runtime: &dyn EnvironmentBackend,
) -> anyhow::Result<String> {
    let image_name = naming::env(build_tree);

    if runtime.env_exists(&image_name)? {
        eprintln!("[image:{image_name}] Already built");
        return Ok(image_name);
    }

    eprintln!("[image:{image_name}] Building...");

    let mut build_args: Vec<(String, String)> = vec![];

    // Build each base environment and pass its key as a build arg so the
    // Containerfile can reference it (e.g. ARG my_base; FROM $my_base).
    let base_entries = meta::read_blob_entries(transaction, odb, build_tree, "bases");
    for (base_name, base_sha) in &base_entries {
        let base_oid = gix_hash::ObjectId::from_str(base_sha.trim())
            .with_context(|| format!("invalid base SHA for {base_name}: {base_sha}"))?;
        let base_env = ensure_image(transaction, odb, base_oid, runtime)?;
        build_args.push((base_name.clone(), base_env));
    }

    for (k, v) in meta::read_blob_entries(transaction, odb, build_tree, "args") {
        build_args.push((k, v));
    }

    let context_entry = tree::read_tree(transaction, odb, build_tree)?
        .entry(b"context")
        .map(|e| e.oid.to_owned())
        .context("workspace image tree missing 'context' subtree")?;

    let context = crate::archive::tree_to_tar(transaction, odb, context_entry)?;

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
