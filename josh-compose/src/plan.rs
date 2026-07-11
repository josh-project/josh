use std::collections::HashSet;

use crate::job_cache;
use crate::meta::{self, OutputMode, WorkspaceMeta};
use crate::podman;

/// Walk the workspace tree and collect every image build-tree OID a run would touch.
///
/// The returned vector is deduplicated and ordered bases-first (a base image's OID
/// always appears before any image that uses it as a base). This is the order in which
/// images would need to be pulled/built for a run to succeed.
///
/// When `ignore_cache` is false (the default), workspaces whose run is already cached
/// successful AND whose output volume is still present are skipped — matching the
/// early-return path in `container::run_container`. When `ignore_cache` is true, every
/// image a fresh-cache run would build is reported.
pub fn collect_image_oids(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    ignore_cache: bool,
) -> anyhow::Result<Vec<git2::Oid>> {
    let mut out: Vec<git2::Oid> = vec![];
    let mut image_seen: HashSet<git2::Oid> = HashSet::new();
    let mut ws_seen: HashSet<git2::Oid> = HashSet::new();
    walk_workspace(
        repo,
        ws_tree,
        ignore_cache,
        &mut out,
        &mut image_seen,
        &mut ws_seen,
    )?;
    Ok(out)
}

/// Walk the workspace tree and collect the job hash (ws_tree OID) of every workspace
/// a run would touch, including orchestrator workspaces that don't build an image —
/// those still write a `job_cache` entry, so they belong in the listing.
///
/// Cache semantics mirror `collect_image_oids`: when `ignore_cache` is false, a
/// workspace whose run is already cached successful AND whose output volume is still
/// present is pruned from the walk. When true, every job a fresh-cache run would
/// touch is reported.
pub fn collect_job_hashes(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    ignore_cache: bool,
) -> anyhow::Result<Vec<git2::Oid>> {
    let mut out: Vec<git2::Oid> = vec![];
    let mut ws_seen: HashSet<git2::Oid> = HashSet::new();
    walk_workspace_jobs(repo, ws_tree, ignore_cache, &mut out, &mut ws_seen)?;
    Ok(out)
}

fn walk_workspace(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    ignore_cache: bool,
    out: &mut Vec<git2::Oid>,
    image_seen: &mut HashSet<git2::Oid>,
    ws_seen: &mut HashSet<git2::Oid>,
) -> anyhow::Result<()> {
    if !ws_seen.insert(ws_tree) {
        return Ok(());
    }

    let workspace_meta = meta::read_meta(repo, ws_tree)?;

    if !ignore_cache && workspace_is_skippable(ws_tree, &workspace_meta)? {
        eprintln!(
            "[{}] Using cached output ({})",
            workspace_meta.label, ws_tree
        );
        return Ok(());
    }

    for (dep_name, dep_sha) in meta::read_blob_entries(repo, ws_tree, "inputs") {
        let dep_tree = git2::Oid::from_str(dep_sha.trim())
            .map_err(|_| anyhow::anyhow!("dependency {dep_name}: invalid tree SHA {dep_sha:?}"))?;
        walk_workspace(repo, dep_tree, ignore_cache, out, image_seen, ws_seen)?;
    }

    if let Some(image_oid) = workspace_meta.image {
        collect_image_with_bases(repo, image_oid, out, image_seen)?;
    }
    for spec in &workspace_meta.sidecars {
        collect_image_with_bases(repo, spec.image, out, image_seen)?;
    }

    Ok(())
}

fn walk_workspace_jobs(
    repo: &git2::Repository,
    ws_tree: git2::Oid,
    ignore_cache: bool,
    out: &mut Vec<git2::Oid>,
    ws_seen: &mut HashSet<git2::Oid>,
) -> anyhow::Result<()> {
    if !ws_seen.insert(ws_tree) {
        return Ok(());
    }

    let workspace_meta = meta::read_meta(repo, ws_tree)?;

    if !ignore_cache && workspace_is_skippable(ws_tree, &workspace_meta)? {
        return Ok(());
    }

    for (dep_name, dep_sha) in meta::read_blob_entries(repo, ws_tree, "inputs") {
        let dep_tree = git2::Oid::from_str(dep_sha.trim())
            .map_err(|_| anyhow::anyhow!("dependency {dep_name}: invalid tree SHA {dep_sha:?}"))?;
        walk_workspace_jobs(repo, dep_tree, ignore_cache, out, ws_seen)?;
    }

    out.push(ws_tree);
    Ok(())
}

/// Mirror `container::run_container`'s early-return condition, tightened to also
/// require the output volume when the workspace produces one. A skippable workspace
/// won't be executed by a run, so its image and sidecar images are not needed.
fn workspace_is_skippable(ws_tree: git2::Oid, meta: &WorkspaceMeta) -> anyhow::Result<bool> {
    let hash = ws_tree.to_string();
    if !job_cache::is_cached_success(&hash) {
        return Ok(false);
    }
    if meta.output == OutputMode::None {
        return Ok(true);
    }
    let out_vol = format!("out_{ws_tree}");
    podman::volume_exists(&out_vol)
}

fn collect_image_with_bases(
    repo: &git2::Repository,
    image_oid: git2::Oid,
    out: &mut Vec<git2::Oid>,
    image_seen: &mut HashSet<git2::Oid>,
) -> anyhow::Result<()> {
    if image_seen.contains(&image_oid) {
        return Ok(());
    }

    for (base_name, base_sha) in meta::read_blob_entries(repo, image_oid, "bases") {
        let base_oid = git2::Oid::from_str(base_sha.trim())
            .map_err(|_| anyhow::anyhow!("invalid base SHA for {base_name}: {base_sha:?}"))?;
        collect_image_with_bases(repo, base_oid, out, image_seen)?;
    }

    if image_seen.insert(image_oid) {
        out.push(image_oid);
    }
    Ok(())
}
