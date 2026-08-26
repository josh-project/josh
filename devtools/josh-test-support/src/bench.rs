//! Repo-building and verification helpers shared by the criterion benches, which
//! generate deterministic histories through [`provision_repo`](crate::provision_repo).

use anyhow::Result;
pub use gix::objs::tree::EntryKind;
use rand::prelude::*;

/// Open a benchmark's gitoxide view from an explicit repository path.
pub fn open_repo(path: impl AsRef<std::path::Path>) -> Result<gix::Repository> {
    Ok(gix::open(path.as_ref())?)
}

/// Deterministic lowercase alphabetic string, used as churned blob content.
pub fn random_string(rng: &mut StdRng, len: usize) -> String {
    (0..len)
        .map(|_| {
            use rand::distr::Alphabetic;
            let ch = Alphabetic.sample(rng) as char;
            ch.to_ascii_lowercase()
        })
        .collect()
}
/// Write the canonical empty tree fixture.
pub fn empty_tree(repo: &gix::Repository) -> Result<gix::ObjectId> {
    Ok(repo
        .write_object(gix::objs::Tree {
            entries: Vec::new(),
        })?
        .detach())
}
/// Resolve a benchmark fixture ref to its direct object target.
pub fn ref_target(repo: &gix::Repository, name: &str) -> Result<gix::ObjectId> {
    Ok(repo.find_reference(name)?.into_fully_peeled_id()?.detach())
}

/// Read a commit's root tree identifier.
pub fn commit_tree(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    josh_gix_ext::CommitData::read(&repo.objects, commit)?.tree_id()
}

/// Resolve `path` from a commit's root tree.
pub fn commit_path(
    repo: &gix::Repository,
    commit: gix::ObjectId,
    path: &std::path::Path,
) -> Result<Option<gix::ObjectId>> {
    let tree = commit_tree(repo, commit)?;
    Ok(josh_gix_ext::path_entry(&repo.objects, tree, path)?.map(|entry| entry.oid))
}

/// Count commits reachable from `head`.
pub fn count_history(repo: &gix::Repository, head: gix::ObjectId) -> Result<usize> {
    let mut walk = josh_gix_ext::RevWalk::new(&repo.objects);
    walk.push(head)?;
    Ok(walk.into_topo_vec(|_| false)?.len())
}

/// Aggregate every case tip under one index commit. Its oid changes whenever any case head
/// changes, making it a faithful content-addressed cache stamp for the entire repo, and it keeps
/// all cases reachable so provision_repo's `git prune` retains the full history. It is never
/// filtered.
pub fn build_index(
    repo: &gix::Repository,
    sig: &gix::actor::Signature,
    heads: &[gix::ObjectId],
) -> Result<gix::ObjectId> {
    let empty_tree = empty_tree(repo)?;
    let index =
        josh_gix_ext::write_commit(&repo.objects, empty_tree, heads, sig, sig, "bench index")?;
    repo.reference(
        "refs/heads/bench-index",
        index,
        gix::refs::transaction::PreviousValue::Any,
        "bench index",
    )?;
    Ok(index)
}

/// Rebuild, with a plain gitoxide tree walk (no josh filtering code), the tree a pattern filter
/// must produce: keep exactly the blobs whose full path satisfies `keep`, at their original paths.
/// Returns the tree oid and the number of kept blobs. Whether a string predicate is an exact
/// stand-in for glob matching is the caller's concern.
pub fn expected_tree(
    repo: &gix::Repository,
    head: gix::ObjectId,
    keep: &dyn Fn(&str) -> bool,
) -> Result<(gix::ObjectId, usize)> {
    let tree = josh_gix_ext::CommitData::read(&repo.objects, head)?.tree_id()?;
    let mut kept = Vec::new();
    josh_gix_ext::walk_tree_preorder(&repo.objects, tree, &mut |parent, entry| {
        let mode = entry.mode.kind();
        if matches!(
            mode,
            EntryKind::Blob | EntryKind::BlobExecutable | EntryKind::Link
        ) {
            let name = std::str::from_utf8(entry.filename)?;
            let path = if parent.is_empty() {
                name.to_owned()
            } else {
                format!("{parent}/{name}")
            };
            if keep(&path) {
                kept.push((path, entry.oid.to_owned(), mode));
            }
        }
        Ok(())
    })?;

    let empty_tree = empty_tree(repo)?;
    let mut builder = repo.edit_tree(empty_tree)?;
    for (path, oid, mode) in &kept {
        builder.upsert(path.as_str(), *mode, *oid)?;
    }
    Ok((builder.write()?.detach(), kept.len()))
}
