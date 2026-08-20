//! Trigram based code search index for git repositories.
//!
//! The index of a tree is itself a git tree: an exact inverted index mapping every trigram
//! (3-byte window of file content, case-folded and with punctuation classes normalized, see
//! [`fold_byte`]) to the set of files containing it. For a trigram with bytes
//! `(b1, b2, b3)`, the index contains
//!
//! ```text
//! <hex(b1)>/<hex(b2)>/<hex(b3)>/path/to/file
//! ```
//!
//! with the empty blob as the leaf marker. The subtree below a trigram's three "spine" levels
//! mirrors the source tree's structure restricted to files containing that trigram.
//!
//! Indexes are built compositionally: each file gets a small trigram tree of its own, child
//! directory indexes are lifted into the parent's namespace, and a directory's children are
//! merged in a single pass. Memoization through [`IndexCache`] (per subtree) and the
//! caller-kept [`Indexer`] state (per blob, wrap and merge) makes reindexing a chain of
//! commits incremental: only what changed is rebuilt. Trees are hashed in memory with
//! [`gix_object`], and only the objects a finished index references are written to the
//! object database.
//!
//! Searching extracts the query's trigrams, resolves each with a single three-level lookup, and
//! intersects the mirror subtrees; the resulting candidate files are exact (files containing all
//! query trigrams), leaving only string-level verification to [`search_matches`].
//!
//! This crate is independent of the josh filter machinery: it operates on plain [`git2`] objects
//! and memoizes tree-to-index mappings through the [`IndexCache`] trait the caller provides.

use gix_object::WriteTo;
use gix_object::bstr::BString;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Memoization of tree oid -> index tree oid mappings, provided by the caller.
///
/// [`trigram_index`] consults this per (sub)tree, which is what makes indexing incremental: when
/// a new commit is indexed, unchanged subtrees hit the cache and reuse their index.
pub trait IndexCache {
    fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid>;
    fn set_index(&self, tree: git2::Oid, index: git2::Oid);
}

fn empty_tree() -> gix_hash::ObjectId {
    gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1)
}

fn empty_blob() -> gix_hash::ObjectId {
    gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1)
}

fn to_gix(oid: git2::Oid) -> gix_hash::ObjectId {
    gix_hash::ObjectId::from_bytes_or_panic(oid.as_bytes())
}

fn to_git2(oid: gix_hash::ObjectId) -> git2::Oid {
    git2::Oid::from_bytes(oid.as_bytes()).expect("oid size mismatch")
}

/// Fold a byte for trigram extraction: ASCII letters lowercase, and every ASCII byte that is
/// not alphanumeric or `_` (whitespace, punctuation, brackets, operators) becomes one class
/// glyph. Folding collapses the combinatorial variety of near-content-free trigrams — on
/// typical source trees it shrinks the index by more than a third — while folded trigrams keep
/// their positional filtering power (a query like `foo(` still requires a non-word byte after
/// `foo`). Non-ASCII bytes pass through untouched, so UTF-8 validity of a window is unaffected.
fn fold_byte(b: u8) -> u8 {
    match b {
        b'A'..=b'Z' => b + 32,
        b if b < 128 && !(b.is_ascii_alphanumeric() || b == b'_') => b' ',
        _ => b,
    }
}

/// All distinct trigrams of `content`: 3-byte windows of the [`fold_byte`]-normalized bytes
/// that are valid UTF-8. Used identically on the index side and the query side, which is what
/// keeps the index exact: folding only merges trigram classes, so a query trigram is found in
/// every file containing the query string, candidates are a superset of the true matches, and
/// [`search_matches`] still verifies the original string byte for byte.
fn distinct_trigrams(content: &str) -> BTreeSet<[u8; 3]> {
    let folded: Vec<u8> = content.bytes().map(fold_byte).collect();
    folded
        .windows(3)
        .filter(|w| std::str::from_utf8(w).is_ok())
        .map(|w| [w[0], w[1], w[2]])
        .collect()
}

/// Read the blob `oid` as text, or "" if it is absent, contains a NUL byte, or is not UTF-8.
fn read_blob_text(src: &dyn Objects, oid: gix_hash::ObjectId) -> String {
    let mut buffer = Vec::new();
    let Ok(Some(data)) = src.try_find(&oid, &mut buffer) else {
        return "".to_owned();
    };
    if data.kind != gix_object::Kind::Blob {
        return "".to_owned();
    }
    if buffer.contains(&0) {
        return "".to_owned();
    }

    let Ok(content) = std::str::from_utf8(&buffer) else {
        return "".to_owned();
    };

    content.to_owned()
}

fn hex_name(b: u8) -> BString {
    format!("{:02x}", b).into()
}

fn tree_entry(filename: BString, oid: gix_hash::ObjectId) -> gix_object::tree::Entry {
    gix_object::tree::Entry {
        mode: gix_object::tree::EntryKind::Tree.into(),
        filename,
        oid,
    }
}

/// Indexing state kept alive across [`trigram_index`] calls (josh keeps one per transaction):
/// unchanged inputs hit the memos instead of being rebuilt. This is what makes indexing a
/// chain of commits incremental beyond the per-tree [`IndexCache`].
///
/// Entries are only ever added, so the sole invariant is that a memoized oid must stay
/// readable — from `pending`, `trees`, or the object database of the repository in use.
/// [`flush`](Run::flush) upholds this by evicting objects only after they are written.
#[derive(Default)]
pub struct Indexer {
    /// Objects not yet written to the ODB. Evicted on flush, so what remains are intermediate
    /// results no persisted index references.
    pending: HashMap<gix_hash::ObjectId, (gix_object::Kind, Vec<u8>)>,
    /// Parsed trees from `pending` and the ODB, so merges don't re-parse the same spine nodes.
    trees: HashMap<gix_hash::ObjectId, std::sync::Arc<gix_object::Tree>>,
    /// Source tree -> index, for trees indexed or cache-resolved with this state.
    tree_memo: HashMap<git2::Oid, gix_hash::ObjectId>,
    /// Source blob and entry name -> the file's wrapped trigram tree. The name is part of the
    /// key because the mirror entries inside carry it.
    blob_memo: HashMap<(git2::Oid, String), gix_hash::ObjectId>,
    wrap_memo: HashMap<(gix_hash::ObjectId, String), gix_hash::ObjectId>,
    overlay_memo: HashMap<Vec<gix_hash::ObjectId>, gix_hash::ObjectId>,
}

/// What an index run needs from the object database.
pub trait Objects: gix_object::Find + gix_object::Exists + gix_object::Write {}
impl<T: gix_object::Find + gix_object::Exists + gix_object::Write> Objects for T {}

/// One [`trigram_index`] call: the persistent [`Indexer`] state plus the per-call object
/// source, cache and root bookkeeping.
struct Run<'a> {
    src: &'a dyn Objects,
    cache: &'a dyn IndexCache,
    ix: &'a mut Indexer,
    /// `(source tree, index)` pairs of this call, memoized by [`flush`](Run::flush) only after
    /// their objects reach the ODB: an [`IndexCache`] entry must never point at missing objects.
    roots: Vec<(git2::Oid, gix_hash::ObjectId)>,
}

impl Run<'_> {
    /// Serialize `entries` as a tree into `pending` and return its hash. Entries are sorted
    /// into git's canonical order (directories compare with a trailing `/`), which differs
    /// from plain name order for the mixed blob/tree entries index trees contain.
    fn write_tree(&mut self, mut entries: Vec<gix_object::tree::Entry>) -> gix_hash::ObjectId {
        if entries.is_empty() {
            return empty_tree();
        }
        entries.sort();
        let tree = gix_object::Tree { entries };
        let mut buffer = Vec::with_capacity(tree.size() as usize);
        tree.write_to(&mut buffer).expect("failed to write tree");
        let hash = gix_object::compute_hash(gix_hash::Kind::Sha1, gix_object::Kind::Tree, &buffer)
            .expect("failed to compute hash");
        if !self.ix.trees.contains_key(&hash) {
            self.ix
                .pending
                .insert(hash, (gix_object::Kind::Tree, buffer));
            self.ix.trees.insert(hash, std::sync::Arc::new(tree));
        }
        hash
    }

    /// Read a tree by hash: from the parsed-tree cache, from `pending`, or from the object
    /// database (e.g. reached through an [`IndexCache`] hit or after a flush evicted it).
    fn read_tree(
        &mut self,
        oid: gix_hash::ObjectId,
    ) -> anyhow::Result<std::sync::Arc<gix_object::Tree>> {
        if let Some(tree) = self.ix.trees.get(&oid) {
            return Ok(tree.clone());
        }
        let tree = if let Some((_, data)) = self.ix.pending.get(&oid) {
            gix_object::TreeRef::from_bytes(data, gix_hash::Kind::Sha1)?.into_owned()
        } else {
            let mut buffer = Vec::new();
            self.src
                .try_find(&oid, &mut buffer)
                .map_err(|e| anyhow::anyhow!("read tree {}: {}", oid, e))?
                .ok_or_else(|| anyhow::anyhow!("object {} not found", oid))?;
            gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?.into_owned()
        };
        let tree = std::sync::Arc::new(tree);
        self.ix.trees.insert(oid, tree.clone());
        Ok(tree)
    }

    /// The wrapped trigram tree of one file: its trigram spine with the one-entry mirror
    /// `{name: empty blob}` at every leaf. Building the wrapped form directly (rather than
    /// wrapping afterwards via [`wrap`](Run::wrap)) avoids an intermediate tree that would
    /// be rewritten anyway.
    fn index_blob(&mut self, oid: git2::Oid, name: &str, content: &str) -> gix_hash::ObjectId {
        let key = (oid, name.to_owned());
        if let Some(id) = self.ix.blob_memo.get(&key) {
            return *id;
        }

        // Grouped by trigram byte and thus already sorted, so write_tree's sort is a no-op.
        let mut spine: BTreeMap<u8, BTreeMap<u8, Vec<u8>>> = BTreeMap::new();
        for t in distinct_trigrams(content) {
            spine
                .entry(t[0])
                .or_default()
                .entry(t[1])
                .or_default()
                .push(t[2]);
        }

        let id = if spine.is_empty() {
            empty_tree()
        } else {
            let mirror = self.write_tree(vec![gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Blob.into(),
                filename: name.into(),
                oid: empty_blob(),
            }]);

            let mut l1 = Vec::new();
            for (b1, l2s) in spine {
                let mut l2 = Vec::new();
                for (b2, b3s) in l2s {
                    let l3 = b3s
                        .into_iter()
                        .map(|b3| tree_entry(hex_name(b3), mirror))
                        .collect();
                    l2.push(tree_entry(hex_name(b2), self.write_tree(l3)));
                }
                l1.push(tree_entry(hex_name(b1), self.write_tree(l2)));
            }
            self.write_tree(l1)
        };

        self.ix.blob_memo.insert(key, id);
        id
    }

    /// Lift a child directory's index into its parent's namespace by nesting each mirror `M`
    /// as the single-entry tree `{name: M}`. Only directories go through here:
    /// [`index_blob`](Run::index_blob) builds the wrapped form of file entries directly.
    fn wrap(
        &mut self,
        name: &str,
        index: gix_hash::ObjectId,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        if index == empty_tree() {
            return Ok(index);
        }
        let key = (index, name.to_owned());
        if let Some(id) = self.ix.wrap_memo.get(&key) {
            return Ok(*id);
        }

        let l1 = self.read_tree(index)?;
        let mut e1 = Vec::with_capacity(l1.entries.len());
        for c1 in l1.entries.iter() {
            let l2 = self.read_tree(c1.oid)?;
            let mut e2 = Vec::with_capacity(l2.entries.len());
            for c2 in l2.entries.iter() {
                let l3 = self.read_tree(c2.oid)?;
                let mut e3 = Vec::with_capacity(l3.entries.len());
                for leaf in l3.entries.iter() {
                    let mirror = vec![gix_object::tree::Entry {
                        mode: leaf.mode,
                        filename: name.into(),
                        oid: leaf.oid,
                    }];
                    e3.push(tree_entry(leaf.filename.clone(), self.write_tree(mirror)));
                }
                e2.push(tree_entry(c2.filename.clone(), self.write_tree(e3)));
            }
            e1.push(tree_entry(c1.filename.clone(), self.write_tree(e2)));
        }

        let id = self.write_tree(e1);
        self.ix.wrap_memo.insert(key, id);
        Ok(id)
    }

    /// Merge index trees, all inputs at once: entries are grouped by name across every input
    /// and each group merges recursively, so every result node is written exactly once (a
    /// pairwise fold would rewrite the accumulated spine once per input). Entries sharing a
    /// name are always trees: they only occur at spine levels, since the wrapped mirrors below
    /// the spine have disjoint names across a directory's children.
    fn overlay_many(
        &mut self,
        mut inputs: Vec<gix_hash::ObjectId>,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        // Union merge: order, duplicates and empty inputs contribute nothing, so normalize
        // the key for maximum memo hits.
        inputs.retain(|id| *id != empty_tree());
        inputs.sort();
        inputs.dedup();
        if inputs.is_empty() {
            return Ok(empty_tree());
        }
        if inputs.len() == 1 {
            return Ok(inputs[0]);
        }
        if let Some(id) = self.ix.overlay_memo.get(&inputs) {
            return Ok(*id);
        }

        let trees = inputs
            .iter()
            .map(|id| self.read_tree(*id))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut groups: BTreeMap<BString, (gix_object::tree::EntryMode, Vec<gix_hash::ObjectId>)> =
            BTreeMap::new();
        for tree in &trees {
            for e in tree.entries.iter() {
                match groups.get_mut(&e.filename) {
                    None => {
                        groups.insert(e.filename.clone(), (e.mode, vec![e.oid]));
                    }
                    Some((mode, oids)) => {
                        anyhow::ensure!(
                            mode.is_tree() && e.mode.is_tree(),
                            "overlay conflict on non-tree entry {:?}",
                            e.filename
                        );
                        oids.push(e.oid);
                    }
                }
            }
        }

        let mut entries = Vec::with_capacity(groups.len());
        for (filename, (mode, oids)) in groups {
            let oid = self.overlay_many(oids)?;
            entries.push(gix_object::tree::Entry {
                mode,
                filename,
                oid,
            });
        }

        let id = self.write_tree(entries);
        self.ix.overlay_memo.insert(inputs, id);
        Ok(id)
    }

    /// The index of a directory: the overlay of its children's wrapped indexes, memoized per
    /// source tree oid. The root is not special — incrementality is just this memo hitting on
    /// unchanged subtrees.
    fn index_tree_oid(&mut self, tree_oid: git2::Oid) -> anyhow::Result<gix_hash::ObjectId> {
        let tree = self.read_tree(to_gix(tree_oid))?;
        if let Some(id) = self.ix.tree_memo.get(&tree_oid) {
            return Ok(*id);
        }
        if let Some(cached) = self.cache.get_index(tree_oid) {
            let id = to_gix(cached);
            self.ix.tree_memo.insert(tree_oid, id);
            return Ok(id);
        }

        let mut wrapped = Vec::with_capacity(tree.entries.len());
        for entry in tree.entries.clone() {
            let name = std::str::from_utf8(&entry.filename)?.to_owned();
            let child_oid = to_git2(entry.oid);
            if entry.mode.is_tree() {
                let child = self.index_tree_oid(child_oid)?;
                wrapped.push(self.wrap(&name, child)?);
            } else if !entry.mode.is_commit() {
                let content = read_blob_text(self.src, entry.oid);
                wrapped.push(self.index_blob(child_oid, &name, &content));
            }
            // Submodules etc. are not indexed.
        }
        let index = self.overlay_many(wrapped)?;

        self.ix.tree_memo.insert(tree_oid, index);
        self.roots.push((tree_oid, index));
        Ok(index)
    }

    /// Write the pending objects reachable from the built indexes to the object database,
    /// then memoize the `(source tree, index)` pairs. Anything still pending afterwards is
    /// an intermediate result and is dropped unwritten.
    fn flush(&mut self) -> anyhow::Result<()> {
        if self.roots.is_empty() {
            return Ok(());
        }
        // Index leaves are the empty blob, and an empty directory's index is the empty tree;
        // neither lives in `pending`, so make sure both exist before anything references them.
        for (kind, id) in [
            (gix_object::Kind::Blob, empty_blob()),
            (gix_object::Kind::Tree, empty_tree()),
        ] {
            if !self.src.exists(&id) {
                self.src
                    .write_buf(kind, &[])
                    .map_err(|e| anyhow::anyhow!("write index object: {}", e))?;
            }
        }

        let mut visited = HashSet::new();
        let mut written = vec![];
        let mut stack: Vec<_> = self.roots.iter().map(|(_, index)| *index).collect();
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            // Not pending: the object (and everything below it) is already in the ODB, either
            // from the start or evicted by an earlier flush.
            let Some((kind, data)) = self.ix.pending.get(&id) else {
                continue;
            };
            if let gix_object::Kind::Tree = kind {
                for entry in gix_object::TreeRef::from_bytes(data, gix_hash::Kind::Sha1)?.entries {
                    stack.push(entry.oid.to_owned());
                }
            }
            if !self.src.exists(&id) {
                self.src
                    .write_buf(*kind, data)
                    .map_err(|e| anyhow::anyhow!("write index object: {}", e))?;
            }
            written.push(id);
        }
        for id in written {
            self.ix.pending.remove(&id);
        }

        for (tree, index) in &self.roots {
            self.cache.set_index(*tree, to_git2(*index));
        }
        Ok(())
    }
}

pub fn trigram_index(
    src: &dyn Objects,
    cache: &dyn IndexCache,
    indexer: &mut Indexer,
    tree: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let mut run = Run {
        src,
        cache,
        ix: indexer,
        roots: Vec::new(),
    };
    let index = run.index_tree_oid(tree)?;
    run.flush()?;
    Ok(to_git2(index))
}

/// Exact candidate files for `searchstring`: files containing every trigram of the query.
///
/// Queries shorter than three bytes (or without any valid-UTF-8 window) have no trigrams; every
/// file of `source_tree` is a candidate then, and [`search_matches`] does the filtering.
pub fn search_candidates(
    src: &dyn Objects,
    index_tree: git2::Oid,
    source_tree: git2::Oid,
    searchstring: &str,
) -> anyhow::Result<Vec<String>> {
    let trigrams = distinct_trigrams(searchstring);

    let mut results = vec![];
    if trigrams.is_empty() {
        collect_paths(src, source_tree, "", &mut results)?;
        return Ok(results);
    }

    let mut roots = vec![];
    for t in &trigrams {
        let path = format!("{:02x}/{:02x}/{:02x}", t[0], t[1], t[2]);
        match path_entry(src, index_tree, std::path::Path::new(&path))? {
            Some(oid) => roots.push(oid),
            // A trigram absent from the index cannot occur in any file.
            None => return Ok(vec![]),
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
            .map(|&oid| anyhow::Ok((read_tree_entries(src, oid)?.len(), oid)))
            .collect::<Result<Vec<_>, _>>()?;
        sized.sort();
        roots = sized
            .into_iter()
            .take(MAX_INTERSECT)
            .map(|(_, oid)| oid)
            .collect();
    }

    intersect_walk(src, &roots, "", &mut results)?;
    Ok(results)
}

/// Emit every file path present in ALL of the mirror trees `roots`.
fn intersect_walk(
    src: &dyn Objects,
    roots: &[git2::Oid],
    prefix: &str,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    // Content addressing fast path: identical mirrors (trigrams with the same file set)
    // intersect to themselves.
    if roots.iter().all(|oid| *oid == roots[0]) {
        return collect_paths(src, roots[0], prefix, out);
    }

    let trees = roots
        .iter()
        .map(|oid| read_tree_entries(src, *oid))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let smallest = trees
        .iter()
        .min_by_key(|t| t.len())
        .expect("roots is never empty");

    'entry: for entry in smallest {
        let name = std::str::from_utf8(&entry.filename)?;

        let mut child_roots = Vec::with_capacity(trees.len());
        for tree in &trees {
            match tree.iter().find(|e| e.filename == entry.filename) {
                Some(other) if other.mode.is_tree() == entry.mode.is_tree() => {
                    child_roots.push(to_git2(other.oid))
                }
                _ => continue 'entry,
            }
        }

        let path = join_path(prefix, name);
        if entry.mode.is_tree() {
            intersect_walk(src, &child_roots, &path, out)?;
        } else if !entry.mode.is_commit() {
            out.push(path);
        }
    }
    Ok(())
}

/// The entries of the tree `oid`, owned so several trees can be walked side by side.
fn read_tree_entries(
    src: &dyn Objects,
    oid: git2::Oid,
) -> anyhow::Result<Vec<gix_object::tree::Entry>> {
    let mut buffer = Vec::new();
    let data = src
        .try_find(&to_gix(oid), &mut buffer)
        .map_err(|e| anyhow::anyhow!("read tree {}: {}", oid, e))?
        .ok_or_else(|| anyhow::anyhow!("object {} not found", oid))?;
    if data.kind != gix_object::Kind::Tree {
        return Err(anyhow::anyhow!("object {} is not a tree", oid));
    }
    Ok(
        gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?
            .into_owned()
            .entries,
    )
}

/// Emit every blob path under `oid` (a tree), prefixed with `prefix`.
fn collect_paths(
    src: &dyn Objects,
    oid: git2::Oid,
    prefix: &str,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    let mut buffer = Vec::new();
    let Some(data) = src
        .try_find(&to_gix(oid), &mut buffer)
        .map_err(|e| anyhow::anyhow!("read tree {}: {}", oid, e))?
    else {
        return Ok(());
    };
    if data.kind != gix_object::Kind::Tree {
        return Ok(());
    }
    let tree = gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?.into_owned();
    for entry in tree.entries {
        let name = std::str::from_utf8(&entry.filename)?;
        let path = join_path(prefix, name);
        if entry.mode.is_tree() {
            collect_paths(src, to_git2(entry.oid), &path, out)?;
        } else if !entry.mode.is_commit() {
            out.push(path);
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
    src: &dyn Objects,
    tree: git2::Oid,
    searchstring: &str,
    candidates: &Vec<String>,
) -> anyhow::Result<SearchMatchesResult> {
    let mut results = vec![];

    for c in candidates {
        let b = get_blob_path(src, tree, std::path::Path::new(&c));

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

/// Like [`read_blob_text`], but for a path inside `tree`.
fn get_blob_path(src: &dyn Objects, tree: git2::Oid, path: &std::path::Path) -> String {
    match path_entry(src, tree, path) {
        Ok(Some(oid)) => read_blob_text(src, to_gix(oid)),
        _ => "".to_owned(),
    }
}

/// The oid at `path` inside `tree`, or `None` when any component is missing or not a tree.
fn path_entry(
    src: &dyn Objects,
    tree: git2::Oid,
    path: &std::path::Path,
) -> anyhow::Result<Option<git2::Oid>> {
    let mut current = tree;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let mut buffer = Vec::new();
        let Some(data) = src
            .try_find(&to_gix(current), &mut buffer)
            .map_err(|e| anyhow::anyhow!("read tree {}: {}", current, e))?
        else {
            return Ok(None);
        };
        if data.kind != gix_object::Kind::Tree {
            return Ok(None);
        }
        let parsed = gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?;
        let name = std::os::unix::ffi::OsStrExt::as_bytes(component.as_os_str());
        let Some(entry) = parsed.entries.iter().find(|e| e.filename == name) else {
            return Ok(None);
        };
        current = to_git2(entry.oid.to_owned());
        if components.peek().is_some() && !entry.mode.is_tree() {
            return Ok(None);
        }
    }
    Ok(Some(current))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Case folds, and all ASCII space/punctuation/bracket bytes are one class glyph;
        // word bytes ([a-z0-9_]) and non-ASCII stay distinct.
        assert_eq!(distinct_trigrams("AbC"), distinct_trigrams("abc"));
        assert_eq!(distinct_trigrams("a,b"), distinct_trigrams("a;b"));
        assert_eq!(distinct_trigrams("f(x)"), distinct_trigrams("f[x]"));
        assert_eq!(distinct_trigrams("a b"), distinct_trigrams("a\tb"));
        assert_ne!(distinct_trigrams("a_b"), distinct_trigrams("a b"));
        assert_ne!(distinct_trigrams("a1b"), distinct_trigrams("a2b"));
    }

    #[derive(Default)]
    struct MapCache {
        map: std::cell::RefCell<std::collections::HashMap<git2::Oid, git2::Oid>>,
    }

    impl IndexCache for MapCache {
        fn get_index(&self, tree: git2::Oid) -> Option<git2::Oid> {
            self.map.borrow().get(&tree).copied()
        }
        fn set_index(&self, tree: git2::Oid, index: git2::Oid) {
            self.map.borrow_mut().insert(tree, index);
        }
    }

    fn test_repo() -> (tempfile::TempDir, git2::Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(tmp.path()).unwrap();
        (tmp, repo)
    }

    /// Object access over a test repository's odb.
    fn objects(repo: &git2::Repository) -> josh_gix_ext::Git2Odb<'_> {
        josh_gix_ext::Git2Odb(Box::leak(Box::new(repo.odb().unwrap())))
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

        let index =
            trigram_index(&objects(&repo), &cache, &mut Indexer::default(), tree.id()).unwrap();

        // The index format is pinned: cached indexes of older josh versions stay valid only as
        // long as this oid does not change.
        assert_eq!(
            index.to_string(),
            "169f9d05072eb25e1f1e90b4f7f11f6cfc2d3222"
        );

        // "Tes" folds to "tes", which lives at 74/65/73 in the hex spine.
        assert!(
            path_entry(
                &objects(&repo),
                index,
                std::path::Path::new("74/65/73/sub1/file1")
            )
            .unwrap()
            .is_some()
        );

        let candidates = search_candidates(&objects(&repo), index, tree.id(), "document").unwrap();
        assert_eq!(candidates, vec!["sub1/file1", "sub1/file2"]);

        // Trigrams are case-folded, so candidates are a case-insensitive superset ("Test" in
        // file1 makes it a candidate for "test") while match verification stays byte-exact.
        let candidates = search_candidates(&objects(&repo), index, tree.id(), "test").unwrap();
        assert_eq!(candidates, vec!["sub1/file1"]);
        let matches = search_matches(&objects(&repo), tree.id(), "test", &candidates).unwrap();
        assert!(matches.is_empty());

        let candidates =
            search_candidates(&objects(&repo), index, tree.id(), "missingword").unwrap();
        assert!(candidates.is_empty());

        // Short query: every file is a candidate.
        let candidates = search_candidates(&objects(&repo), index, tree.id(), "e").unwrap();
        assert_eq!(candidates.len(), 3);

        // Indexing is deterministic and memoization-independent: a cold rebuild of the same
        // tree yields the same oid.
        let cold = MapCache::default();
        let index2 =
            trigram_index(&objects(&repo), &cold, &mut Indexer::default(), tree.id()).unwrap();
        assert_eq!(index, index2);
    }

    #[test]
    fn incremental_index_equals_cold_build() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();

        // One Indexer shared across both commits, like josh keeps one per transaction: the
        // second call must reuse blob/wrap/merge state from the first.
        let mut indexer = Indexer::default();

        let tree_a = commit_tree(
            &repo,
            &[
                ("sub1/keep", "the quick brown fox"),
                ("sub1/gone", "unique zebra content"),
                ("sub2/mod", "alpha beta gamma"),
            ],
        );
        trigram_index(&objects(&repo), &cache, &mut indexer, tree_a.id()).unwrap();

        // One file modified, one removed (its unique trigrams must vanish from the spine), one
        // added in a fresh directory.
        let tree_b = commit_tree(
            &repo,
            &[
                ("sub1/keep", "the quick brown fox"),
                ("sub2/mod", "alpha beta delta"),
                ("sub3/new", "fresh addition here"),
            ],
        );
        let index_b = trigram_index(&objects(&repo), &cache, &mut indexer, tree_b.id()).unwrap();

        // Every subtree is memoized, including the fresh one — incrementality has no special
        // path that could skip it.
        let sub3 = tree_b.get_name("sub3").unwrap().id();
        assert!(cache.get_index(sub3).is_some());

        // Warm (incremental) and cold-built indexes agree bit for bit.
        let cold = MapCache::default();
        let index_b_cold =
            trigram_index(&objects(&repo), &cold, &mut Indexer::default(), tree_b.id()).unwrap();
        assert_eq!(index_b, index_b_cold);

        // The incremental index searches correctly.
        let hits = search_candidates(&objects(&repo), index_b, tree_b.id(), "delta").unwrap();
        assert_eq!(hits, vec!["sub2/mod"]);
        let hits = search_candidates(&objects(&repo), index_b, tree_b.id(), "zebra").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(&objects(&repo), index_b, tree_b.id(), "addition").unwrap();
        assert_eq!(hits, vec!["sub3/new"]);
    }
}
