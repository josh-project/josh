//! Repo-building and verification helpers shared by the criterion benches, which
//! generate deterministic histories through [`provision_repo`](crate::provision_repo).

use std::path::Path;

use anyhow::Result;
use rand::prelude::*;

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

/// Aggregate every case tip under one index commit. Its oid changes whenever any case head
/// changes, making it a faithful content-addressed cache stamp for the entire repo, and it keeps
/// all cases reachable so provision_repo's `git prune` retains the full history. It is never
/// filtered.
pub fn build_index(
    repo: &git2::Repository,
    sig: &git2::Signature,
    heads: &[git2::Oid],
) -> Result<git2::Oid> {
    let empty_tree = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let parents = heads
        .iter()
        .map(|oid| repo.find_commit(*oid))
        .collect::<Result<Vec<_>, _>>()?;
    let parent_refs = parents.iter().collect::<Vec<_>>();
    let index = repo.commit(
        Some("refs/heads/bench-index"),
        sig,
        sig,
        "bench index",
        &empty_tree,
        &parent_refs,
    )?;
    Ok(index)
}

/// Rebuild, with plain git2 tree walking (no josh code), the tree a pattern filter must produce:
/// keep exactly the blobs whose full path satisfies `keep`, at their ORIGINAL paths (a pattern
/// filter preserves paths; it does not lift subtrees to the root). Returns the tree oid and the
/// number of kept blobs. Whether a string predicate is an exact stand-in for glob matching is the
/// caller's concern (dot-leading path components never match `*`/`**` under
/// `require_literal_leading_dot`; a glob-based predicate is exact regardless).
pub fn expected_tree(
    repo: &git2::Repository,
    head: git2::Oid,
    keep: &dyn Fn(&str) -> bool,
) -> Result<(git2::Oid, usize)> {
    let tree = repo.find_commit(head)?.tree()?;
    let mut kept: Vec<(String, git2::Oid, i32)> = vec![];
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let path = format!("{}{}", root, entry.name().unwrap_or_default());
            if keep(&path) {
                kept.push((path, entry.id(), entry.filemode()));
            }
        }
        git2::TreeWalkResult::Ok
    })?;
    let mut builder = git2::build::TreeUpdateBuilder::new();
    for (path, oid, filemode) in &kept {
        let mode = match *filemode {
            0o100755 => git2::FileMode::BlobExecutable,
            0o120000 => git2::FileMode::Link,
            _ => git2::FileMode::Blob,
        };
        builder.upsert(Path::new(path), *oid, mode);
    }
    let baseline = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    Ok((builder.create_updated(repo, &baseline)?, kept.len()))
}
