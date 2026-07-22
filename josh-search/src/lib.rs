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
            let child = trigram_index(repo, cache, repo.find_tree(entry.id())?)?;
            for_each_spine_leaf(repo, &child, |t, mirror| {
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

pub fn trigram_index<'a>(
    repo: &'a git2::Repository,
    cache: &dyn IndexCache,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    if let Some(cached) = cache.get_index(tree.id()) {
        return Ok(repo.find_tree(cached)?);
    }
    let empty_blob = repo.blob(b"")?;
    let result = build_index(repo, cache, &tree, empty_blob)?;
    cache.set_index(tree.id(), result);
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

    struct MapCache(std::cell::RefCell<std::collections::HashMap<git2::Oid, git2::Oid>>);

    impl IndexCache for MapCache {
        fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
            self.0.borrow().get(&tree).copied()
        }
        fn set_index(&self, tree: git2::Oid, index: git2::Oid) {
            self.0.borrow_mut().insert(tree, index);
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
        let cache = MapCache(Default::default());

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
        let cold = MapCache(Default::default());
        let index2 = trigram_index(&repo, &cold, tree).unwrap();
        assert_eq!(index.id(), index2.id());
    }
}
