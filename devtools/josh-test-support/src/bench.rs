//! Repo-building and verification helpers shared by the criterion benches, which
//! generate deterministic histories through [`provision_repo`](crate::provision_repo).

use anyhow::Result;
pub use gix::objs::tree::EntryKind;
use rand::prelude::*;

/// Convert the SHA-1 identifier used by the remaining benchmark APIs to gitoxide.
pub fn gix_oid(oid: git2::Oid) -> gix::ObjectId {
    gix::ObjectId::from_bytes_or_panic(oid.as_bytes())
}

/// Convert a gitoxide SHA-1 identifier for the remaining benchmark APIs.
pub fn git2_oid(oid: impl AsRef<gix::hash::oid>) -> git2::Oid {
    git2::Oid::from_bytes(oid.as_ref().as_bytes()).expect("gitoxide repository uses SHA-1")
}
/// Open a benchmark's libgit2 reference-model view without repository discovery.
pub fn open_git2_repo(path: impl AsRef<std::path::Path>) -> Result<git2::Repository> {
    Ok(git2::Repository::open_ext(
        path,
        git2::RepositoryOpenFlags::NO_SEARCH,
        &[] as &[&std::ffi::OsStr],
    )?)
}

/// Josh's fixed libgit2 identity for deterministic benchmark fixtures.
pub fn josh_commit_signature<'a>() -> Result<git2::Signature<'a>> {
    const NAME: &str = "JOSH";
    const EMAIL: &str = "josh@josh-project.dev";

    Ok(match std::env::var("JOSH_COMMIT_TIME") {
        Ok(time) => git2::Signature::new(NAME, EMAIL, &git2::Time::new(time.parse()?, 0))?,
        Err(_) => git2::Signature::now(NAME, EMAIL)?,
    })
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

/// Aggregate every case tip under one index commit. Its oid changes whenever any case head
/// changes, making it a faithful content-addressed cache stamp for the entire repo, and it keeps
/// all cases reachable so provision_repo's `git prune` retains the full history. It is never
/// filtered.
pub fn build_index(
    repo: &git2::Repository,
    sig: &git2::Signature,
    heads: &[gix::ObjectId],
) -> Result<gix::ObjectId> {
    let empty_tree = repo.find_tree(repo.treebuilder(None)?.write()?)?;
    let parents = heads
        .iter()
        .map(|oid| repo.find_commit(git2_oid(*oid)))
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
    Ok(gix_oid(index))
}

/// Rebuild, with plain git2 tree walking (no josh code), the tree a pattern filter must produce:
/// keep exactly the blobs whose full path satisfies `keep`, at their ORIGINAL paths (a pattern
/// filter preserves paths; it does not lift subtrees to the root). Returns the tree oid and the
/// number of kept blobs. Whether a string predicate is an exact stand-in for glob matching is the
/// caller's concern (dot-leading path components never match `*`/`**` under
/// `require_literal_leading_dot`; a glob-based predicate is exact regardless).
pub fn expected_tree(
    repo: &git2::Repository,
    head: gix::ObjectId,
    keep: &dyn Fn(&str) -> bool,
) -> Result<(gix::ObjectId, usize)> {
    let tree = repo.find_commit(git2_oid(head))?.tree()?;
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
    let baseline = repo.treebuilder(None)?.write()?;
    let gix_repo = gix::open(repo.path())?;
    let mut builder = gix_repo.edit_tree(gix_oid(baseline))?;
    for (path, oid, filemode) in &kept {
        let mode = match *filemode {
            0o100755 => EntryKind::BlobExecutable,
            0o120000 => EntryKind::Link,
            _ => EntryKind::Blob,
        };
        builder.upsert(path.as_str(), mode, gix_oid(*oid))?;
    }
    Ok((builder.write()?.detach(), kept.len()))
}
