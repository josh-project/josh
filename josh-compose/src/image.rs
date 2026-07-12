use anyhow::Context;

use josh_compose_backend::{EnvRecipe, Runtime};

use crate::meta;
use crate::naming;

/// Ensure the environment for the given build tree exists, building it if needed.
/// Returns the environment key (image name).
pub fn ensure_image(
    repo: &git2::Repository,
    build_tree: git2::Oid,
    runtime: &dyn Runtime,
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
    let base_entries = meta::read_blob_entries(repo, build_tree, "bases");
    for (base_name, base_sha) in &base_entries {
        let base_oid = git2::Oid::from_str(base_sha.trim())
            .with_context(|| format!("invalid base SHA for {base_name}: {base_sha}"))?;
        let base_env = ensure_image(repo, base_oid, runtime)?;
        build_args.push((base_name.clone(), base_env));
    }

    for (k, v) in meta::read_blob_entries(repo, build_tree, "args") {
        build_args.push((k, v));
    }

    let tree = repo.find_tree(build_tree)?;
    let context_entry = tree
        .get_path(std::path::Path::new("context"))
        .context("workspace image tree missing 'context' subtree")?;
    let context_oid = context_entry.id();

    let context = crate::archive::tree_to_tar(repo, context_oid)?;

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
