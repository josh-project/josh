//! Trigram based code search index for git repositories.
//!
//! The index of a tree is itself a git tree: an exact inverted index mapping every trigram
//! (3-byte window of file content) to the set of files containing it. For a trigram with bytes
//! `(b1, b2, b3)`, the index contains
//!
//! ```text
//! <hex(b1)>/<hex(b2)>/<hex(b3)>/path/to/file
//! ```
//!
//! with the empty blob as the leaf marker. The subtree below a trigram's three "spine" levels
//! mirrors the source tree's structure restricted to files containing that trigram. Mirrors are
//! built compositionally — a directory's mirror references its children's mirrors by oid — so
//! git's content addressing shares identical file sets across trigrams and across commits.
//!
//! Searching extracts the query's trigrams, resolves each with a single three-level lookup, and
//! intersects the mirror subtrees; the resulting candidate files are exact (files containing all
//! query trigrams), leaving only string-level verification to [`search_matches`].
//!
//! This crate is independent of the josh filter machinery: it operates on plain [`git2`] objects
//! and memoizes tree-to-index mappings through the [`IndexCache`] trait the caller provides.

use anyhow::anyhow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const BLOB_MODE: i32 = 0o0100644;
const TREE_MODE: i32 = 0o0040000;

/// Memoization of tree oid -> index tree oid mappings, provided by the caller.
///
/// [`trigram_index`] consults this per (sub)tree, which is what makes indexing incremental: when
/// a new commit is indexed, unchanged subtrees hit the cache and reuse their index.
pub trait IndexCache {
    fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid>;
    fn set_index(&self, tree: git2::Oid, index: git2::Oid);

    /// Recently indexed `(tree, index)` pairs, most recent first. [`trigram_index`] uses them as
    /// diff bases: when the tree to index differs from a recent one in only a few files, the
    /// recent index is patched instead of rebuilt. The default (no memory) disables that fast
    /// path; correctness does not depend on it.
    fn get_recent(&self) -> Vec<(git2::Oid, git2::Oid)> {
        vec![]
    }
    fn set_recent(&self, _tree: git2::Oid, _index: git2::Oid) {}
}

fn empty_tree_id() -> git2::Oid {
    git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap()
}

/// All distinct trigrams of `content`: 3-byte windows that are valid UTF-8. Used identically on
/// the index side and the query side, which is what makes the index exact — UTF-8 validity of a
/// window depends only on its own bytes, so a query trigram is found in every file containing
/// the query string.
fn distinct_trigrams(content: &str) -> BTreeSet<[u8; 3]> {
    content
        .as_bytes()
        .windows(3)
        .filter(|w| std::str::from_utf8(w).is_ok())
        .map(|w| [w[0], w[1], w[2]])
        .collect()
}

fn decode_hex_name(name: &str) -> Option<u8> {
    // Spines are written with lowercase hex only; reject anything else (incl. uppercase) so
    // encoding and decoding stay bijective.
    if name.len() != 2
        || !name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    u8::from_str_radix(name, 16).ok()
}

/// Read the blob at `name` in `tree` as text, or "" if it is absent, binary or not UTF-8.
fn get_blob(repo: &git2::Repository, tree: &git2::Tree, name: &str) -> String {
    let Some(entry) = tree.get_name(name) else {
        return "".to_owned();
    };

    let Ok(blob) = repo.find_blob(entry.id()) else {
        return "".to_owned();
    };

    if blob.is_binary() {
        return "".to_owned();
    }

    let Ok(content) = std::str::from_utf8(blob.content()) else {
        return "".to_owned();
    };

    content.to_owned()
}

/// Per-directory posting map: trigram -> (entry name -> (oid, filemode)). For an own file the
/// oid is the empty blob; for a child directory it is the child's mirror subtree for that
/// trigram.
type Postings = BTreeMap<[u8; 3], BTreeMap<String, (git2::Oid, i32)>>;

/// Iterate the `(trigram, mirror_oid)` leaves of an index tree's spine.
fn for_each_spine_leaf(
    repo: &git2::Repository,
    index: &git2::Tree,
    mut f: impl FnMut([u8; 3], git2::Oid) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for e1 in index.iter() {
        let b1 = e1
            .name()
            .and_then(decode_hex_name)
            .ok_or_else(|| anyhow!("invalid spine entry"))?;
        let t1 = repo.find_tree(e1.id())?;
        for e2 in t1.iter() {
            let b2 = e2
                .name()
                .and_then(decode_hex_name)
                .ok_or_else(|| anyhow!("invalid spine entry"))?;
            let t2 = repo.find_tree(e2.id())?;
            for e3 in t2.iter() {
                let b3 = e3
                    .name()
                    .and_then(decode_hex_name)
                    .ok_or_else(|| anyhow!("invalid spine entry"))?;
                f([b1, b2, b3], e3.id())?;
            }
        }
    }
    Ok(())
}

/// Write one tree from `(name, oid, filemode)` entries.
fn write_tree(
    repo: &git2::Repository,
    entries: &[(String, git2::Oid, i32)],
) -> anyhow::Result<git2::Oid> {
    let mut builder = repo.treebuilder(None)?;
    for (name, oid, mode) in entries {
        builder.insert(name, *oid, *mode)?;
    }
    Ok(builder.write()?)
}

/// Build the index of `tree`'s postings: the c1/c2/c3 spine with each trigram's mirror subtree
/// below it. Mirrors identical across trigrams (same file set) are written once and shared.
fn write_spine(repo: &git2::Repository, postings: &Postings) -> anyhow::Result<git2::Oid> {
    if postings.is_empty() {
        return Ok(empty_tree_id());
    }

    // trigram mirror oids, deduped by entry list: trigrams occurring in the same set of files
    // share one mirror tree.
    let mut mirror_cache: HashMap<Vec<(String, git2::Oid, i32)>, git2::Oid> = HashMap::new();

    // b1 -> b2 -> b3 -> mirror oid
    let mut spine: BTreeMap<u8, BTreeMap<u8, Vec<(String, git2::Oid, i32)>>> = BTreeMap::new();
    for (t, files) in postings {
        let entries: Vec<_> = files
            .iter()
            .map(|(name, (oid, mode))| (name.clone(), *oid, *mode))
            .collect();
        let mirror = match mirror_cache.get(&entries) {
            Some(oid) => *oid,
            None => {
                let oid = write_tree(repo, &entries)?;
                mirror_cache.insert(entries, oid);
                oid
            }
        };
        spine
            .entry(t[0])
            .or_default()
            .entry(t[1])
            .or_default()
            .push((format!("{:02x}", t[2]), mirror, TREE_MODE));
    }

    let mut c1_entries = vec![];
    for (b1, c2s) in &spine {
        let mut c2_entries = vec![];
        for (b2, c3s) in c2s {
            c2_entries.push((format!("{:02x}", b2), write_tree(repo, c3s)?, TREE_MODE));
        }
        c1_entries.push((
            format!("{:02x}", b1),
            write_tree(repo, &c2_entries)?,
            TREE_MODE,
        ));
    }
    write_tree(repo, &c1_entries)
}

/// Build the index of one directory: own files contribute empty-blob postings, child
/// directories contribute their (memoized) index's mirror subtrees by oid.
fn build_index(
    repo: &git2::Repository,
    cache: &dyn IndexCache,
    tree: &git2::Tree,
    empty_blob: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let mut postings = Postings::new();

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            let content = get_blob(repo, tree, name);
            for t in distinct_trigrams(&content) {
                postings
                    .entry(t)
                    .or_default()
                    .insert(name.to_owned(), (empty_blob, BLOB_MODE));
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let child = index_subtree(repo, cache, &repo.find_tree(entry.id())?, empty_blob)?;
            for_each_spine_leaf(repo, &repo.find_tree(child)?, |t, mirror| {
                postings
                    .entry(t)
                    .or_default()
                    .insert(name.to_owned(), (mirror, TREE_MODE));
                Ok(())
            })?;
        }
    }

    write_spine(repo, &postings)
}

/// Memoized recursive indexing of a subtree. Unlike the public [`trigram_index`], this never
/// attempts the diff fast path and never touches the recent ring: subtrees are memoized per oid
/// and are not useful diff bases.
fn index_subtree(
    repo: &git2::Repository,
    cache: &dyn IndexCache,
    tree: &git2::Tree,
    empty_blob: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    if let Some(cached) = cache.get_index(tree.id()) {
        return Ok(cached);
    }
    let result = build_index(repo, cache, tree, empty_blob)?;
    cache.set_index(tree.id(), result);
    Ok(result)
}

/// Trigrams of the blob `oid`, subject to the same gate as [`get_blob`] (binary or non-UTF-8
/// content indexes as empty). Must stay in sync with how [`build_index`] reads file content, or
/// the diff fast path diverges from cold builds.
fn blob_trigrams(repo: &git2::Repository, oid: git2::Oid) -> BTreeSet<[u8; 3]> {
    let Ok(blob) = repo.find_blob(oid) else {
        return BTreeSet::new();
    };
    if blob.is_binary() {
        return BTreeSet::new();
    }
    let Ok(content) = std::str::from_utf8(blob.content()) else {
        return BTreeSet::new();
    };
    distinct_trigrams(content)
}

/// A blob changed between two source trees: its path, and its oid on the old and new side
/// (`None` = absent on that side).
type BlobChange = (String, Option<git2::Oid>, Option<git2::Oid>);

/// Emit every blob under `oid` as one-sided [`BlobChange`]s (`old` side if `as_old`).
fn collect_blob_changes(
    repo: &git2::Repository,
    oid: git2::Oid,
    prefix: &str,
    as_old: bool,
    out: &mut Vec<BlobChange>,
) -> anyhow::Result<()> {
    let tree = repo.find_tree(oid)?;
    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
        let path = join_path(prefix, name);
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                collect_blob_changes(repo, entry.id(), &path, as_old, out)?
            }
            Some(git2::ObjectType::Blob) => out.push(if as_old {
                (path, Some(entry.id()), None)
            } else {
                (path, None, Some(entry.id()))
            }),
            _ => {}
        }
    }
    Ok(())
}

/// Collect the blobs differing between the trees `old` and `new` into `out`. Returns false
/// (giving up on the diff) once more than `limit` changed blobs accumulate.
fn tree_diff(
    repo: &git2::Repository,
    old: git2::Oid,
    new: git2::Oid,
    limit: usize,
    prefix: &str,
    out: &mut Vec<BlobChange>,
) -> anyhow::Result<bool> {
    if old == new {
        return Ok(true);
    }
    let old_tree = repo.find_tree(old)?;
    let new_tree = repo.find_tree(new)?;

    // Entries the indexer cares about, by name: blobs and trees only (submodules etc. are not
    // indexed and diff as absent).
    let relevant = |tree: &git2::Tree| -> BTreeMap<String, (git2::Oid, git2::ObjectType)> {
        tree.iter()
            .filter_map(|e| match (e.name(), e.kind()) {
                (Some(name), Some(kind))
                    if kind == git2::ObjectType::Blob || kind == git2::ObjectType::Tree =>
                {
                    Some((name.to_owned(), (e.id(), kind)))
                }
                _ => None,
            })
            .collect()
    };
    let old_entries = relevant(&old_tree);
    let new_entries = relevant(&new_tree);

    let names: BTreeSet<&String> = old_entries.keys().chain(new_entries.keys()).collect();
    for name in names {
        let path = join_path(prefix, name);
        use git2::ObjectType::{Blob, Tree};
        match (old_entries.get(name), new_entries.get(name)) {
            (Some((a, ka)), Some((b, kb))) if a == b && ka == kb => {}
            (Some((a, Tree)), Some((b, Tree))) => {
                if !tree_diff(repo, *a, *b, limit, &path, out)? {
                    return Ok(false);
                }
            }
            (Some((a, Blob)), Some((b, Blob))) => out.push((path, Some(*a), Some(*b))),
            // One-sided or kind-changed: everything on the old side is removed, everything on
            // the new side added.
            (old_side, new_side) => {
                match old_side {
                    Some((a, Blob)) => out.push((path.clone(), Some(*a), None)),
                    Some((a, Tree)) => collect_blob_changes(repo, *a, &path, true, out)?,
                    _ => {}
                }
                match new_side {
                    Some((b, Blob)) => out.push((path.clone(), None, Some(*b))),
                    Some((b, Tree)) => collect_blob_changes(repo, *b, &path, false, out)?,
                    _ => {}
                }
            }
        }
        if out.len() > limit {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Insert (`present`) or remove the empty-blob leaf at `path` inside the mirror tree `tree_oid`,
/// creating intermediate directories on insert and pruning directories that become empty on
/// removal. Returns the new tree oid; `empty_tree_id()` means the subtree vanished entirely and
/// the caller must drop its entry (the canonical form never contains empty directories).
fn set_leaf(
    repo: &git2::Repository,
    tree_oid: git2::Oid,
    path: &[&str],
    present: bool,
    empty_blob: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let tree = repo.find_tree(tree_oid)?;
    let (name, rest) = path.split_first().expect("path is never empty");
    let mut builder = repo.treebuilder(Some(&tree))?;

    if rest.is_empty() {
        if present {
            builder.insert(name, empty_blob, BLOB_MODE)?;
        } else if tree.get_name(name).is_some() {
            builder.remove(name)?;
        }
    } else {
        let child = tree
            .get_name(name)
            .map(|e| e.id())
            .unwrap_or_else(empty_tree_id);
        let child = set_leaf(repo, child, rest, present, empty_blob)?;
        if child == empty_tree_id() {
            if tree.get_name(name).is_some() {
                builder.remove(name)?;
            }
        } else {
            builder.insert(name, child, TREE_MODE)?;
        }
    }
    Ok(builder.write()?)
}

/// Produce the index of a tree that differs from an already indexed one by `changes`, patching
/// `prev_index` posting by posting. Produces the same canonical form as a cold build, so the
/// resulting oid is bit-identical to what [`build_index`] would return.
fn patch_index(
    repo: &git2::Repository,
    prev_index: git2::Oid,
    changes: &[BlobChange],
    empty_blob: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    // Make sure the empty tree object exists: set_leaf and the spine patching below look it up.
    repo.treebuilder(None)?.write()?;

    // trigram -> (path, add/remove), grouped so every touched spine node is rewritten once.
    let mut edits: BTreeMap<[u8; 3], Vec<(&str, bool)>> = BTreeMap::new();
    for (path, old_oid, new_oid) in changes {
        let old_t = old_oid.map(|o| blob_trigrams(repo, o)).unwrap_or_default();
        let new_t = new_oid.map(|o| blob_trigrams(repo, o)).unwrap_or_default();
        for t in old_t.difference(&new_t) {
            edits.entry(*t).or_default().push((path, false));
        }
        for t in new_t.difference(&old_t) {
            edits.entry(*t).or_default().push((path, true));
        }
    }

    let index_tree = repo.find_tree(prev_index)?;
    let mut c1_builder = repo.treebuilder(Some(&index_tree))?;

    let mut edits = edits.into_iter().peekable();
    while let Some(&([b1, _, _], _)) = edits.peek() {
        let c1_name = format!("{:02x}", b1);
        let c1_oid = index_tree
            .get_name(&c1_name)
            .map(|e| e.id())
            .unwrap_or_else(empty_tree_id);
        let c1_tree = repo.find_tree(c1_oid)?;
        let mut c2_builder = repo.treebuilder(Some(&c1_tree))?;

        while let Some(&([e1, b2, _], _)) = edits.peek() {
            if e1 != b1 {
                break;
            }
            let c2_name = format!("{:02x}", b2);
            let c2_oid = c1_tree
                .get_name(&c2_name)
                .map(|e| e.id())
                .unwrap_or_else(empty_tree_id);
            let c2_tree = repo.find_tree(c2_oid)?;
            let mut c3_builder = repo.treebuilder(Some(&c2_tree))?;

            while let Some(&([e1, e2, b3], _)) = edits.peek() {
                if e1 != b1 || e2 != b2 {
                    break;
                }
                let (_, path_edits) = edits.next().unwrap();
                let c3_name = format!("{:02x}", b3);
                let mut mirror = c2_tree
                    .get_name(&c3_name)
                    .map(|e| e.id())
                    .unwrap_or_else(empty_tree_id);
                for (path, present) in path_edits {
                    let components: Vec<&str> = path.split('/').collect();
                    mirror = set_leaf(repo, mirror, &components, present, empty_blob)?;
                }
                if mirror == empty_tree_id() {
                    if c2_tree.get_name(&c3_name).is_some() {
                        c3_builder.remove(&c3_name)?;
                    }
                } else {
                    c3_builder.insert(&c3_name, mirror, TREE_MODE)?;
                }
            }

            let new_c2 = c3_builder.write()?;
            if new_c2 == empty_tree_id() {
                if c1_tree.get_name(&c2_name).is_some() {
                    c2_builder.remove(&c2_name)?;
                }
            } else {
                c2_builder.insert(&c2_name, new_c2, TREE_MODE)?;
            }
        }

        let new_c1 = c2_builder.write()?;
        if new_c1 == empty_tree_id() {
            if index_tree.get_name(&c1_name).is_some() {
                c1_builder.remove(&c1_name)?;
            }
        } else {
            c1_builder.insert(&c1_name, new_c1, TREE_MODE)?;
        }
    }

    Ok(c1_builder.write()?)
}

pub fn trigram_index<'a>(
    repo: &'a git2::Repository,
    cache: &dyn IndexCache,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    if let Some(cached) = cache.get_index(tree.id()) {
        return Ok(repo.find_tree(cached)?);
    }
    let empty_blob = repo.blob(b"")?;

    // Fast path: if the tree differs from a recently indexed one in only a few files, patch
    // that index instead of rebuilding. Only tried at the root — the recursion below deals in
    // subtrees, which are memoized per oid and never good diff bases.
    const DIFF_LIMIT: usize = 256;
    let mut result = None;
    for (prev_tree, prev_index) in cache.get_recent() {
        let mut changes = vec![];
        if prev_tree != tree.id()
            && tree_diff(repo, prev_tree, tree.id(), DIFF_LIMIT, "", &mut changes)?
        {
            result = Some(patch_index(repo, prev_index, &changes, empty_blob)?);
            break;
        }
    }

    let result = match result {
        Some(result) => result,
        None => build_index(repo, cache, &tree, empty_blob)?,
    };

    cache.set_index(tree.id(), result);
    cache.set_recent(tree.id(), result);
    Ok(repo.find_tree(result)?)
}

/// Exact candidate files for `searchstring`: files containing every trigram of the query.
///
/// Queries shorter than three bytes (or without any valid-UTF-8 window) have no trigrams; every
/// file of `source_tree` is a candidate then, and [`search_matches`] does the filtering.
pub fn search_candidates(
    repo: &git2::Repository,
    index_tree: &git2::Tree,
    source_tree: &git2::Tree,
    searchstring: &str,
) -> anyhow::Result<Vec<String>> {
    let trigrams = distinct_trigrams(searchstring);

    let mut results = vec![];
    if trigrams.is_empty() {
        collect_paths(repo, source_tree.id(), "", &mut results)?;
        return Ok(results);
    }

    let mut roots = vec![];
    for t in &trigrams {
        let path = format!("{:02x}/{:02x}/{:02x}", t[0], t[1], t[2]);
        match index_tree.get_path(std::path::Path::new(&path)) {
            Ok(entry) => roots.push(entry.id()),
            // A trigram absent from the index cannot occur in any file.
            Err(_) => return Ok(vec![]),
        }
    }
    roots.sort();
    roots.dedup();

    // Cap the intersection width: any subset of trigrams yields a superset of candidates, and
    // search_matches verifies exactly anyway. Keep the mirrors with the fewest entries — they
    // constrain the most.
    const MAX_INTERSECT: usize = 16;
    if roots.len() > MAX_INTERSECT {
        let mut sized = roots
            .iter()
            .map(|&oid| anyhow::Ok((repo.find_tree(oid)?.len(), oid)))
            .collect::<Result<Vec<_>, _>>()?;
        sized.sort();
        roots = sized
            .into_iter()
            .take(MAX_INTERSECT)
            .map(|(_, oid)| oid)
            .collect();
    }

    intersect_walk(repo, &roots, "", &mut results)?;
    Ok(results)
}

/// Emit every file path present in ALL of the mirror trees `roots`.
fn intersect_walk(
    repo: &git2::Repository,
    roots: &[git2::Oid],
    prefix: &str,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    // Content addressing fast path: identical mirrors (trigrams with the same file set)
    // intersect to themselves.
    if roots.iter().all(|oid| *oid == roots[0]) {
        return collect_paths(repo, roots[0], prefix, out);
    }

    let trees = roots
        .iter()
        .map(|oid| repo.find_tree(*oid))
        .collect::<Result<Vec<_>, _>>()?;
    let smallest = trees
        .iter()
        .min_by_key(|t| t.len())
        .expect("roots is never empty");

    'entry: for entry in smallest.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;

        let mut child_roots = Vec::with_capacity(trees.len());
        for tree in &trees {
            match tree.get_name(name) {
                Some(other) if other.kind() == entry.kind() => child_roots.push(other.id()),
                _ => continue 'entry,
            }
        }

        let path = join_path(prefix, name);
        match entry.kind() {
            Some(git2::ObjectType::Tree) => intersect_walk(repo, &child_roots, &path, out)?,
            Some(git2::ObjectType::Blob) => out.push(path),
            _ => {}
        }
    }
    Ok(())
}

/// Emit every blob path under `oid` (a tree), prefixed with `prefix`.
fn collect_paths(
    repo: &git2::Repository,
    oid: git2::Oid,
    prefix: &str,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    let tree = repo.find_tree(oid)?;
    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
        let path = join_path(prefix, name);
        match entry.kind() {
            Some(git2::ObjectType::Tree) => collect_paths(repo, entry.id(), &path, out)?,
            Some(git2::ObjectType::Blob) => out.push(path),
            _ => {}
        }
    }
    Ok(())
}

fn join_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{}", prefix, name)
    }
}

type SearchMatchesResult = Vec<(String, Vec<(usize, String)>)>;

pub fn search_matches(
    repo: &git2::Repository,
    tree: &git2::Tree,
    searchstring: &str,
    candidates: &Vec<String>,
) -> anyhow::Result<SearchMatchesResult> {
    let mut results = vec![];

    for c in candidates {
        let b = get_blob_path(repo, tree, std::path::Path::new(&c));

        let mut bresults = vec![];

        for (linenr, l) in b.lines().enumerate() {
            if l.contains(searchstring) {
                bresults.push((linenr + 1, l.to_owned()));
            }
        }

        if !bresults.is_empty() {
            results.push((c.to_owned(), bresults));
        }
    }

    Ok(results)
}

/// Like [`get_blob`], but for a (possibly nested) path instead of a root entry name.
fn get_blob_path(repo: &git2::Repository, tree: &git2::Tree, path: &std::path::Path) -> String {
    let Ok(entry) = tree.get_path(path) else {
        return "".to_owned();
    };

    let Ok(blob) = repo.find_blob(entry.id()) else {
        return "".to_owned();
    };

    if blob.is_binary() {
        return "".to_owned();
    }

    let Ok(content) = std::str::from_utf8(blob.content()) else {
        return "".to_owned();
    };

    content.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_names_round_trip() {
        for b in 0..=255u8 {
            assert_eq!(decode_hex_name(&format!("{:02x}", b)), Some(b));
        }
        assert_eq!(decode_hex_name("g0"), None);
        assert_eq!(decode_hex_name("0"), None);
        assert_eq!(decode_hex_name("000"), None);
        // Uppercase never appears in written spines; reject to keep encode/decode bijective.
        assert_eq!(decode_hex_name("0A"), None);
    }

    #[test]
    fn distinct_trigrams_basics() {
        assert!(distinct_trigrams("").is_empty());
        assert!(distinct_trigrams("ab").is_empty());
        assert_eq!(
            distinct_trigrams("abc"),
            BTreeSet::from([[b'a', b'b', b'c']])
        );
        // Repeated windows collapse.
        assert_eq!(distinct_trigrams("aaaa"), BTreeSet::from([[b'a'; 3]]));
        // Multibyte: only valid UTF-8 windows are kept. "é" is 2 bytes; the windows straddling
        // its bytes are not valid UTF-8 strings except where they align.
        let t = distinct_trigrams("aéb");
        assert!(t.contains(&[b'a', 0xc3, 0xa9]));
        assert!(t.contains(&[0xc3, 0xa9, b'b']));
        assert_eq!(t.len(), 2);
    }

    #[derive(Default)]
    struct MapCache {
        map: std::cell::RefCell<std::collections::HashMap<git2::Oid, git2::Oid>>,
        recent: std::cell::RefCell<Vec<(git2::Oid, git2::Oid)>>,
    }

    impl IndexCache for MapCache {
        fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
            self.map.borrow().get(&tree).copied()
        }
        fn set_index(&self, tree: git2::Oid, index: git2::Oid) {
            self.map.borrow_mut().insert(tree, index);
        }
        fn get_recent(&self) -> Vec<(git2::Oid, git2::Oid)> {
            self.recent.borrow().clone()
        }
        fn set_recent(&self, tree: git2::Oid, index: git2::Oid) {
            self.recent.borrow_mut().insert(0, (tree, index));
        }
    }

    fn test_repo() -> (tempfile::TempDir, git2::Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(tmp.path()).unwrap();
        (tmp, repo)
    }

    fn commit_tree<'a>(repo: &'a git2::Repository, files: &[(&str, &str)]) -> git2::Tree<'a> {
        let mut builder = git2::build::TreeUpdateBuilder::new();
        for (path, content) in files {
            let oid = repo.blob(content.as_bytes()).unwrap();
            builder.upsert(std::path::Path::new(path), oid, git2::FileMode::Blob);
        }
        let baseline = repo
            .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        let oid = builder.create_updated(repo, &baseline).unwrap();
        repo.find_tree(oid).unwrap()
    }

    #[test]
    fn index_and_search_end_to_end() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();

        let tree = commit_tree(
            &repo,
            &[
                ("sub1/file1", "First Test document"),
                ("sub1/file2", "Another document"),
                ("sub2/file3", "One more to see what happens"),
            ],
        );

        let index = trigram_index(&repo, &cache, tree.clone()).unwrap();

        // "Tes" lives at 54/65/73 ('T' escapes nothing -- hex spine).
        assert!(
            index
                .get_path(std::path::Path::new("54/65/73/sub1/file1"))
                .is_ok()
        );

        let candidates = search_candidates(&repo, &index, &tree, "document").unwrap();
        assert_eq!(candidates, vec!["sub1/file1", "sub1/file2"]);

        let candidates = search_candidates(&repo, &index, &tree, "missingword").unwrap();
        assert!(candidates.is_empty());

        // Short query: every file is a candidate.
        let candidates = search_candidates(&repo, &index, &tree, "e").unwrap();
        assert_eq!(candidates.len(), 3);

        // Indexing is deterministic and memoization-independent: a cold rebuild of the same
        // tree yields the same oid.
        let cold = MapCache::default();
        let index2 = trigram_index(&repo, &cold, tree).unwrap();
        assert_eq!(index.id(), index2.id());
    }

    #[test]
    fn patched_index_equals_cold_build() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();

        let tree_a = commit_tree(
            &repo,
            &[
                ("sub1/keep", "the quick brown fox"),
                ("sub1/gone", "unique zebra content"),
                ("sub2/mod", "alpha beta gamma"),
            ],
        );
        trigram_index(&repo, &cache, tree_a).unwrap();
        assert_eq!(cache.get_recent().len(), 1);

        // One file modified, one removed (its unique trigrams must be pruned from the spine),
        // one added in a fresh directory.
        let tree_b = commit_tree(
            &repo,
            &[
                ("sub1/keep", "the quick brown fox"),
                ("sub2/mod", "alpha beta delta"),
                ("sub3/new", "fresh addition here"),
            ],
        );
        let index_b = trigram_index(&repo, &cache, tree_b.clone()).unwrap();

        // The diff fast path was actually taken: a cold build would have recursed and memoized
        // sub3's subtree index, the patch path never does.
        let sub3 = tree_b.get_name("sub3").unwrap().id();
        assert!(cache.get_index(sub3).is_none());

        // Patched and cold-built indexes agree bit for bit.
        let cold = MapCache::default();
        let index_b_cold = trigram_index(&repo, &cold, tree_b.clone()).unwrap();
        assert_eq!(index_b.id(), index_b_cold.id());

        // And the patched index searches correctly.
        let hits = search_candidates(&repo, &index_b, &tree_b, "delta").unwrap();
        assert_eq!(hits, vec!["sub2/mod"]);
        let hits = search_candidates(&repo, &index_b, &tree_b, "zebra").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(&repo, &index_b, &tree_b, "addition").unwrap();
        assert_eq!(hits, vec!["sub3/new"]);
    }
}
