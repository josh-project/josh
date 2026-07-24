//! Trigram based code search index for git repositories.
//!
//! The index of a tree is itself a git tree: an exact inverted index mapping every trigram
//! (3-byte window of file content, case-folded and with punctuation classes normalized, see
//! [`fold_byte`]) to the set of files containing it. For a trigram with bytes
//! `(b1, b2, b3)`, the index contains
//!
//! ```text
//! <hex(b1)>/<hex(b2)>/<hex(b3)>/<bucket>/<bucket>...
//! ```
//!
//! with the empty blob as the leaf marker. The subtree below a trigram's three "spine" levels
//! mirrors the source tree's structure restricted to files containing that trigram. Mirror
//! entries are named by a one-byte hash of the entry's name (two hex chars, see
//! [`bucket_name`]) rather than by the name itself: two bytes instead of full names, fan-out
//! capped at 256, and — because the hash depends only on the entry's own name — mirrors stay
//! stable when siblings are added or removed. Colliding names share a bucket, which then
//! means "some member contains the trigram"; search expands buckets to all their members via
//! the source tree ([`bucketed_entries`]), keeping candidates a superset and verification
//! exact.
//!
//! Mirror granularity is adaptive: small directories (see [`COARSE_MAX_FILES`] /
//! [`COARSE_MAX_BYTES`]) appear as a single blob leaf — "some file under this directory
//! contains the trigram" — instead of per-file structure, collapsing the per-trigram file
//! subset variety where it is cheapest to re-verify. Search expands such a coarse hit to every
//! file under the directory, so candidates stay a superset and verification stays exact.
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

/// The mirror entry name of a source entry: a one-byte FNV-1a hash of its name, as two hex
/// chars. Depending only on the entry's own name keeps mirrors stable when siblings come and
/// go (a dense scheme like positions would rename neighbours on every insertion), while
/// cutting entry names to two bytes and capping mirror fan-out at 256. Colliding names share
/// a bucket, which then means "some member contains the trigram".
fn bucket_byte(name: &[u8]) -> u8 {
    let mut h: u32 = 0x811c9dc5;
    for &b in name {
        h = (h ^ b as u32).wrapping_mul(0x01000193);
    }
    (h ^ (h >> 8) ^ (h >> 16) ^ (h >> 24)) as u8
}

fn bucket_name(name: &[u8]) -> String {
    format!("{:02x}", bucket_byte(name))
}

/// A byte as two lowercase hex chars — the fixed-width entry-name form used by both the spine
/// and the mirror bucket levels.
fn hex_pair(b: u8) -> [u8; 2] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    [HEX[(b >> 4) as usize], HEX[(b & 15) as usize]]
}

/// Parse a mirror entry name (two lowercase hex chars) back into its bucket byte.
fn parse_bucket(name: &[u8]) -> Option<u8> {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        }
    }
    match name {
        [hi, lo] => Some(hex_val(*hi)? * 16 + hex_val(*lo)?),
        _ => None,
    }
}

/// A source directory's indexable entries with their mirror bucket names: blobs and trees in
/// git tree order; other kinds (submodules etc.) are not indexed. Bucket names repeat on
/// collision. The index side and the search side must both enumerate exactly this way — it is
/// the bucket -> entries mapping, derived from the source tree instead of stored in the index.
fn bucketed_entries(
    entries: &[gix_object::tree::Entry],
) -> Vec<(String, &gix_object::tree::Entry)> {
    entries
        .iter()
        .filter(|e| !e.mode.is_commit())
        .map(|e| (bucket_name(&e.filename), e))
        .collect()
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
    /// Source blob and bucket -> the file's wrapped trigram tree. The bucket is part of the
    /// key because the mirror entries inside carry it.
    blob_memo: HashMap<(git2::Oid, String), gix_hash::ObjectId>,
    /// Source blob -> its nameless trigram spine (empty-blob leaves), the building block of
    /// coarse indexes. Name-independent, so identical blobs share it everywhere.
    blob_spine_memo: HashMap<git2::Oid, gix_hash::ObjectId>,
    /// Source tree -> its coarse index (the subtree treated as one pseudo-file). Kept apart
    /// from the fine-grained [`IndexCache`]; coarse subtrees are small by definition, so
    /// keeping this per process is cheap enough.
    coarse_memo: HashMap<git2::Oid, gix_hash::ObjectId>,
    /// Source tree -> transitive `(file count, content bytes)`, for the granularity decision.
    stats_memo: HashMap<git2::Oid, (u64, u64)>,
    wrap_memo: HashMap<(gix_hash::ObjectId, String), gix_hash::ObjectId>,
    overlay_memo: HashMap<Vec<gix_hash::ObjectId>, gix_hash::ObjectId>,
}

/// What an index run needs from the object database.
pub trait Objects:
    gix_object::Find + gix_object::FindHeader + gix_object::Exists + gix_object::Write
{
}
impl<T: gix_object::Find + gix_object::FindHeader + gix_object::Exists + gix_object::Write> Objects
    for T
{
}

/// Directories at or below BOTH limits are recorded at directory granularity: their mirrors
/// hold one leaf for the whole directory instead of one per file. Search then verifies every
/// file under a matched coarse directory, so the limits bound that extra verification work.
const COARSE_MAX_FILES: u64 = 16;
const COARSE_MAX_BYTES: u64 = 64 * 1024;

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

    /// Build the spine of `content`'s trigrams with `(leaf_mode, leaf_oid)` at every
    /// `hex(b1)/hex(b2)/hex(b3)` leaf, or the empty tree when there are no trigrams.
    fn content_spine(
        &mut self,
        content: &str,
        leaf_mode: gix_object::tree::EntryMode,
        leaf_oid: gix_hash::ObjectId,
    ) -> gix_hash::ObjectId {
        // b1 -> b2 -> [b3]; already sorted, write_tree's canonical sort is a no-op here.
        let mut spine: BTreeMap<u8, BTreeMap<u8, Vec<u8>>> = BTreeMap::new();
        for t in distinct_trigrams(content) {
            spine
                .entry(t[0])
                .or_default()
                .entry(t[1])
                .or_default()
                .push(t[2]);
        }
        if spine.is_empty() {
            return empty_tree();
        }

        let mut l1 = Vec::new();
        for (b1, l2s) in spine {
            let mut l2 = Vec::new();
            for (b2, b3s) in l2s {
                let l3 = b3s
                    .into_iter()
                    .map(|b3| gix_object::tree::Entry {
                        mode: leaf_mode,
                        filename: hex_name(b3),
                        oid: leaf_oid,
                    })
                    .collect();
                l2.push(tree_entry(hex_name(b2), self.write_tree(l3)));
            }
            l1.push(tree_entry(hex_name(b1), self.write_tree(l2)));
        }
        self.write_tree(l1)
    }

    /// The wrapped trigram tree of one file: its trigram spine with the one-entry mirror
    /// `{bucket: empty blob}` at every leaf. Building the wrapped form directly (rather than
    /// wrapping afterwards via [`wrap`](Run::wrap)) avoids an intermediate tree that would
    /// be rewritten anyway.
    fn index_blob(&mut self, oid: git2::Oid, bucket: &str, content: &str) -> gix_hash::ObjectId {
        let key = (oid, bucket.to_owned());
        if let Some(id) = self.ix.blob_memo.get(&key) {
            return *id;
        }

        let mirror = self.write_tree(vec![gix_object::tree::Entry {
            mode: gix_object::tree::EntryKind::Blob.into(),
            filename: bucket.into(),
            oid: empty_blob(),
        }]);
        let id = self.content_spine(content, gix_object::tree::EntryKind::Tree.into(), mirror);

        self.ix.blob_memo.insert(key, id);
        id
    }

    /// The nameless trigram spine of one blob: empty-blob leaves, no mirror. The building
    /// block of coarse (directory granularity) indexes.
    fn blob_spine(&mut self, oid: git2::Oid, content: &str) -> gix_hash::ObjectId {
        if let Some(id) = self.ix.blob_spine_memo.get(&oid) {
            return *id;
        }
        let id = self.content_spine(
            content,
            gix_object::tree::EntryKind::Blob.into(),
            empty_blob(),
        );
        self.ix.blob_spine_memo.insert(oid, id);
        id
    }

    /// Transitive `(file count, content bytes)` of a tree, for the granularity decision.
    fn subtree_stats(&mut self, tree_oid: git2::Oid) -> anyhow::Result<(u64, u64)> {
        if let Some(stats) = self.ix.stats_memo.get(&tree_oid) {
            return Ok(*stats);
        }
        let tree = self.read_tree(to_gix(tree_oid))?;
        let (mut files, mut bytes) = (0u64, 0u64);
        for entry in tree.entries.clone() {
            if entry.mode.is_tree() {
                let (f, b) = self.subtree_stats(to_git2(entry.oid))?;
                files += f;
                bytes += b;
            } else if !entry.mode.is_commit() {
                files += 1;
                bytes += self
                    .src
                    .try_header(&entry.oid)
                    .map_err(|e| anyhow::anyhow!("read header {}: {}", entry.oid, e))?
                    .ok_or_else(|| anyhow::anyhow!("object {} not found", entry.oid))?
                    .size;
            }
        }
        self.ix.stats_memo.insert(tree_oid, (files, bytes));
        Ok((files, bytes))
    }

    /// Whether a child directory is recorded at directory granularity by its parent.
    fn is_small(&mut self, tree_oid: git2::Oid) -> anyhow::Result<bool> {
        let (files, bytes) = self.subtree_stats(tree_oid)?;
        Ok(files <= COARSE_MAX_FILES && bytes <= COARSE_MAX_BYTES)
    }

    /// The coarse index of a tree: the whole subtree treated as one pseudo-file — the spine of
    /// every trigram occurring anywhere below it, with empty-blob leaves and no mirror
    /// structure. Wrapping it under the directory's name yields `{name: empty blob}` mirrors:
    /// a blob leaf where the source has a directory, meaning "some file under here contains
    /// the trigram".
    fn coarse_index(&mut self, tree_oid: git2::Oid) -> anyhow::Result<gix_hash::ObjectId> {
        if let Some(id) = self.ix.coarse_memo.get(&tree_oid) {
            return Ok(*id);
        }

        let tree = self.read_tree(to_gix(tree_oid))?;
        let mut spines = Vec::with_capacity(tree.entries.len());
        for entry in tree.entries.clone() {
            if entry.mode.is_tree() {
                spines.push(self.coarse_index(to_git2(entry.oid))?);
            } else if !entry.mode.is_commit() {
                let content = read_blob_text(self.src, entry.oid);
                spines.push(self.blob_spine(to_git2(entry.oid), &content));
            }
        }
        let index = self.overlay_many(spines)?;

        self.ix.coarse_memo.insert(tree_oid, index);
        Ok(index)
    }

    /// Lift a child directory's index into its parent's namespace by nesting each mirror `M`
    /// as the single-entry tree `{bucket: M}`. Only directories go through here:
    /// [`index_blob`](Run::index_blob) builds the wrapped form of file entries directly.
    fn wrap(
        &mut self,
        bucket: &str,
        index: gix_hash::ObjectId,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        if index == empty_tree() {
            return Ok(index);
        }
        let key = (index, bucket.to_owned());
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
                        filename: bucket.into(),
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

    /// Merge index trees, all inputs at once, so every result node is written exactly once (a
    /// pairwise fold would rewrite the accumulated spine once per input). Same-name entries
    /// are either trees — spine levels, or two children hashing to the same bucket — which
    /// merge recursively, or identical empty-blob leaves, which collapse.
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
                            (mode.is_tree() && e.mode.is_tree())
                                || (*mode == e.mode && oids[0] == e.oid),
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

        // Mirror entries are named by bucket (name hash); bucketed_entries is the mapping and
        // the search side derives the same one from the source tree. First decide what each
        // entry contributes: files and small directories contribute blob leaves, large
        // directories mirror subtrees — except in a bucket that mixes both kinds, where the
        // large directories degrade to coarse leaves so the bucket's entries stay mergeable.
        let entries = bucketed_entries(&tree.entries);
        let mut leaf_buckets = HashSet::new();
        for (bucket, entry) in &entries {
            let is_leaf = if entry.mode.is_tree() {
                self.is_small(to_git2(entry.oid))?
            } else {
                true
            };
            if is_leaf {
                leaf_buckets.insert(bucket.clone());
            }
        }

        let mut wrapped = Vec::with_capacity(entries.len());
        for (bucket, entry) in &entries {
            if entry.mode.is_tree() {
                let child_oid = to_git2(entry.oid);
                // Small directories are recorded at directory granularity: one coarse
                // leaf for the whole directory instead of per-file mirrors. Large ones
                // too when their bucket also holds leaf contributions.
                let coarse = self.is_small(child_oid)? || leaf_buckets.contains(bucket);
                let child = if coarse {
                    self.coarse_index(child_oid)?
                } else {
                    self.index_tree_oid(child_oid)?
                };
                wrapped.push(self.wrap(bucket, child)?);
            } else {
                let content = read_blob_text(self.src, entry.oid);
                wrapped.push(self.index_blob(to_git2(entry.oid), bucket, &content));
            }
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

/// Search memoization, meant to be kept alive for many [`search_candidates`] /
/// [`search_matches`] calls (josh keeps one per transaction). Everything is keyed by content
/// — object ids and the query string — so entries are valid for any commit of the repository:
/// searching a history reuses the candidate walks of every shared subtree and verifies every
/// distinct blob once, no matter how many commits it appears in.
#[derive(Default)]
pub struct SearchCache {
    /// (query, index tree, source tree) -> sorted candidate (path, blob) pairs.
    candidates: HashMap<(String, git2::Oid, git2::Oid), std::sync::Arc<Vec<(String, git2::Oid)>>>,
    /// (mirror roots (normalized), source tree) -> relative candidate (path, blob) pairs.
    walks: HashMap<(Vec<git2::Oid>, git2::Oid), std::sync::Arc<Vec<(String, git2::Oid)>>>,
    /// Mirror tree oid -> its entries in compact parsed form. Mirrors are shared heavily
    /// across commits and trigrams, and libgit2 re-parses trees on every lookup — this cache
    /// parses each distinct mirror once per process.
    mirrors: HashMap<git2::Oid, std::sync::Arc<Vec<MirrorEntry>>>,
    /// source tree -> all (relative path, blob) pairs under it.
    all_paths: HashMap<git2::Oid, std::sync::Arc<Vec<(String, git2::Oid)>>>,
    /// (query, blob) -> matching (line number, line) pairs.
    blob_matches: HashMap<(String, git2::Oid), std::sync::Arc<Vec<(usize, String)>>>,
}

/// The candidate files for `searchstring`: those containing every trigram of the query.
///
/// Queries shorter than three characters have no trigrams; every file of `source_tree` is a
/// candidate then, and [`search_matches`] does the filtering.
pub fn search_candidates(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: git2::Oid,
    source_tree: git2::Oid,
    searchstring: &str,
) -> anyhow::Result<Vec<(String, git2::Oid)>> {
    let key = (searchstring.to_owned(), index_tree, source_tree);
    if let Some(hit) = cache.candidates.get(&key) {
        return Ok((**hit).clone());
    }

    let trigrams = distinct_trigrams(searchstring);

    let results = if trigrams.is_empty() {
        (*all_paths(src, cache, source_tree)?).clone()
    } else {
        // Resolve each trigram's three spine levels through the parsed-mirror cache (spine
        // names are the same fixed-width hex as bucket names): each distinct spine node is
        // parsed once per process instead of once per lookup.
        let mut roots = vec![];
        let mut absent = false;
        'trigram: for t in &trigrams {
            let mut node = index_tree;
            for b in t {
                let entries = mirror_entries(src, cache, node)?;
                match entries.binary_search_by_key(&hex_pair(*b), |e| e.name) {
                    Ok(i) => node = entries[i].oid,
                    // A trigram absent from the index cannot occur in any file.
                    Err(_) => {
                        absent = true;
                        break 'trigram;
                    }
                }
            }
            roots.push(node);
        }
        if absent {
            vec![]
        } else {
            roots.sort();
            roots.dedup();

            // Cap the intersection width: any subset of trigrams yields a superset of
            // candidates, and search_matches verifies exactly anyway. Keep the mirrors with
            // the fewest entries — they constrain the most.
            const MAX_INTERSECT: usize = 16;
            if roots.len() > MAX_INTERSECT {
                let mut sized = roots
                    .iter()
                    .map(|&oid| anyhow::Ok((mirror_entries(src, cache, oid)?.len(), oid)))
                    .collect::<Result<Vec<_>, _>>()?;
                sized.sort();
                roots = sized
                    .into_iter()
                    .take(MAX_INTERSECT)
                    .map(|(_, oid)| oid)
                    .collect();
            }

            (*walk(src, cache, roots, source_tree)?).clone()
        }
    };

    cache
        .candidates
        .insert(key, std::sync::Arc::new(results.clone()));
    Ok(results)
}

/// One mirror tree entry in compact parsed form: the fixed-width bucket name, the child oid,
/// and whether it is a subtree (versus a coarse or file leaf).
struct MirrorEntry {
    name: [u8; 2],
    oid: git2::Oid,
    tree: bool,
}

/// The entries of the mirror tree `oid`, parsed once per process and memoized.
fn mirror_entries(
    src: &dyn Objects,
    cache: &mut SearchCache,
    oid: git2::Oid,
) -> anyhow::Result<std::sync::Arc<Vec<MirrorEntry>>> {
    if let Some(hit) = cache.mirrors.get(&oid) {
        return Ok(hit.clone());
    }
    let mut entries = vec![];
    for entry in read_tree_entries(src, oid)? {
        if let [a, b] = entry.filename.as_slice() {
            entries.push(MirrorEntry {
                name: [*a, *b],
                oid: to_git2(entry.oid),
                tree: entry.mode.is_tree(),
            });
        }
    }
    let entries = std::sync::Arc::new(entries);
    cache.mirrors.insert(oid, entries.clone());
    Ok(entries)
}

/// The candidate file paths (relative to `source`) present in ALL of the mirror trees
/// `roots`. Mirror entries are named by bucket; `source` provides the bucket -> entries
/// mapping per level. A blob entry expands to every bucket member (each file directly, each
/// directory — a coarse leaf — to all files under it); a tree entry recurses into every
/// large-directory member. Results are relative and keyed by content, so a walk is reused
/// across commits and trigrams wherever the subtrees agree.
///
/// The intersection is a k-way merge over the mirrors' entry lists: bucket names are fixed
/// width, so git's canonical entry order is plain byte order and one linear pass replaces
/// per-bucket lookups. This loop runs once per commit of a history sweep, so its constant
/// factor matters.
fn walk(
    src: &dyn Objects,
    cache: &mut SearchCache,
    mut roots: Vec<git2::Oid>,
    source: git2::Oid,
) -> anyhow::Result<std::sync::Arc<Vec<(String, git2::Oid)>>> {
    // The intersection is a set operation: normalize the key. Identical mirrors (trigrams
    // with the same file set) intersect to themselves, so duplicates collapse.
    roots.sort();
    roots.dedup();
    let key = (roots.clone(), source);
    if let Some(hit) = cache.walks.get(&key) {
        return Ok(hit.clone());
    }

    let entry_lists = roots
        .iter()
        .map(|oid| mirror_entries(src, cache, *oid))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Source entries by bucket byte; a flat table instead of a hash map keyed by name.
    let source_entries = read_tree_entries(src, source)?;
    let mut by_bucket: Vec<Vec<&gix_object::tree::Entry>> = (0..256).map(|_| Vec::new()).collect();
    for entry in &source_entries {
        if !entry.mode.is_commit() {
            by_bucket[bucket_byte(&entry.filename) as usize].push(entry);
        }
    }

    let k = entry_lists.len();
    let mut idx = vec![0usize; k];
    let mut out = vec![];
    'merge: loop {
        // The largest bucket name under the cursors; every list must reach it for the bucket
        // to be in the intersection.
        let mut max: [u8; 2] = match entry_lists[0].get(idx[0]) {
            Some(e) => e.name,
            None => break,
        };
        for i in 1..k {
            match entry_lists[i].get(idx[i]) {
                Some(e) if e.name > max => max = e.name,
                Some(_) => {}
                None => break 'merge,
            }
        }
        let mut all_equal = true;
        for i in 0..k {
            while entry_lists[i].get(idx[i]).is_some_and(|e| e.name < max) {
                idx[i] += 1;
            }
            match entry_lists[i].get(idx[i]) {
                Some(e) if e.name == max => {}
                Some(_) => all_equal = false,
                None => break 'merge,
            }
        }
        if !all_equal {
            continue;
        }

        let is_tree = entry_lists[0][idx[0]].tree;
        let kinds_match = (1..k).all(|i| entry_lists[i][idx[i]].tree == is_tree);
        if kinds_match {
            let members = parse_bucket(&max)
                .map(|b| &by_bucket[b as usize][..])
                .unwrap_or(&[]);
            if is_tree {
                let child_roots: Vec<git2::Oid> =
                    (0..k).map(|i| entry_lists[i][idx[i]].oid).collect();
                for member in members {
                    if !member.mode.is_tree() {
                        continue;
                    }
                    let name = std::str::from_utf8(&member.filename)?;
                    let sub = walk(src, cache, child_roots.clone(), to_git2(member.oid))?;
                    out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
                }
            } else {
                for member in members {
                    let name = std::str::from_utf8(&member.filename)?;
                    if member.mode.is_tree() {
                        let sub = all_paths(src, cache, to_git2(member.oid))?;
                        out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
                    } else if !member.mode.is_commit() {
                        out.push((name.to_owned(), to_git2(member.oid)));
                    }
                }
            }
        }
        for i in 0..k {
            idx[i] += 1;
        }
    }

    // Mirror iteration follows bucket (hash) order; keep results in path order.
    out.sort();
    out.dedup();
    let out = std::sync::Arc::new(out);
    cache.walks.insert(key, out.clone());
    Ok(out)
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

/// All (relative path, blob) pairs under the tree `oid`, memoized per tree: the expansion of
/// coarse leaves and the fallback for queries without trigrams.
fn all_paths(
    src: &dyn Objects,
    cache: &mut SearchCache,
    oid: git2::Oid,
) -> anyhow::Result<std::sync::Arc<Vec<(String, git2::Oid)>>> {
    if let Some(hit) = cache.all_paths.get(&oid) {
        return Ok(hit.clone());
    }
    let mut out = vec![];
    for entry in read_tree_entries(src, oid)? {
        let name = std::str::from_utf8(&entry.filename)?;
        if entry.mode.is_tree() {
            let sub = all_paths(src, cache, to_git2(entry.oid))?;
            out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
        } else if !entry.mode.is_commit() {
            out.push((name.to_owned(), to_git2(entry.oid)));
        }
    }
    let out = std::sync::Arc::new(out);
    cache.all_paths.insert(oid, out.clone());
    Ok(out)
}

fn join_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{}", prefix, name)
    }
}

type SearchMatchesResult = Vec<(String, Vec<(usize, String)>)>;

/// Verify `candidates` against the query, byte-exact. Per-blob results are memoized on
/// (query, blob oid): a blob's matching lines do not depend on the commit or path it appears
/// under, so verifying a history costs one scan per distinct blob. Candidates carry their
/// blob oids from candidate selection, so no path resolution happens here.
pub fn search_matches(
    src: &dyn Objects,
    cache: &mut SearchCache,
    searchstring: &str,
    candidates: &[(String, git2::Oid)],
) -> anyhow::Result<SearchMatchesResult> {
    let mut results = vec![];

    for (c, blob) in candidates {
        let key = (searchstring.to_owned(), *blob);
        let bresults = if let Some(hit) = cache.blob_matches.get(&key) {
            hit.clone()
        } else {
            let b = read_blob_text(src, to_gix(*blob));
            let mut lines = vec![];
            for (linenr, l) in b.lines().enumerate() {
                if l.contains(searchstring) {
                    lines.push((linenr + 1, l.to_owned()));
                }
            }
            let lines = std::sync::Arc::new(lines);
            cache.blob_matches.insert(key, lines.clone());
            lines
        };

        if !bresults.is_empty() {
            results.push((c.to_owned(), (*bresults).clone()));
        }
    }

    Ok(results)
}

/// The oid at `path` inside `tree`, or `None` when any component is missing or not a tree.
/// Production code resolves spine and mirror levels through [`mirror_entries`]; this remains
/// as the tests' direct way to probe index paths.
#[cfg(test)]
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
            "03111ef5ca624b955d799a80181c1bf0c2099fcd"
        );

        let mut sc = SearchCache::default();

        // "Tes" folds to "tes", which lives at 74/65/73 in the hex spine. sub1 is a small
        // directory, so the mirror records it as one coarse blob leaf under its bucket name.
        let leaf = path_entry(
            &objects(&repo),
            index,
            std::path::Path::new(&format!("74/65/73/{}", bucket_name(b"sub1"))),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            repo.find_object(leaf, None).unwrap().kind(),
            Some(git2::ObjectType::Blob)
        );

        // Coarse hits expand to every file under the directory; verification is exact.
        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "document").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(&objects(&repo), &mut sc, "document", &candidates).unwrap();
        assert_eq!(matches.len(), 2);

        // Trigrams are case-folded, so candidates are a case-insensitive superset ("Test" in
        // file1 makes sub1 a candidate for "test") while match verification stays byte-exact.
        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "test").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(&objects(&repo), &mut sc, "test", &candidates).unwrap();
        assert!(matches.is_empty());

        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "missingword").unwrap();
        assert!(candidates.is_empty());

        // Short query: every file is a candidate.
        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "e").unwrap();
        assert_eq!(candidates.len(), 3);

        // Indexing is deterministic and memoization-independent.
        let cold = MapCache::default();
        let index2 =
            trigram_index(&objects(&repo), &cold, &mut Indexer::default(), tree.id()).unwrap();
        assert_eq!(index, index2);
    }

    #[test]
    fn bucket_name_basics() {
        for name in ["a", "file.txt", "sub1", "ütf8"] {
            let b = bucket_name(name.as_bytes());
            assert_eq!(b.len(), 2);
            assert!(b.bytes().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(b, bucket_name(name.as_bytes()));
        }
    }

    #[test]
    fn colliding_names_share_a_bucket() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();

        // Find a name whose bucket collides with "target_file".
        let target_bucket = bucket_name(b"target_file");
        let collider = (0..100_000)
            .map(|i| format!("other{}", i))
            .find(|n| bucket_name(n.as_bytes()) == target_bucket)
            .expect("collision exists among 100k candidates");

        // Enough filler files to keep the directory fine-grained.
        let mut files: Vec<(String, String)> = (0..18)
            .map(|i| (format!("big/filler_{:02}", i), format!("filler {}", i)))
            .collect();
        files.push((
            "big/target_file".to_owned(),
            "needleword lives here".to_owned(),
        ));
        files.push((format!("big/{}", collider), "collider content".to_owned()));
        let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (&p[..], &c[..])).collect();
        let tree = commit_tree(&repo, &files);

        let index =
            trigram_index(&objects(&repo), &cache, &mut Indexer::default(), tree.id()).unwrap();

        // The bucket is a superset: both members are candidates for a needle in one of them;
        // verification is exact.
        let mut sc = SearchCache::default();
        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "needleword").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"big/target_file"));
        assert!(paths.contains(&format!("big/{}", collider).as_str()));
        let matches = search_matches(&objects(&repo), &mut sc, "needleword", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "big/target_file");
    }

    #[test]
    fn coarse_and_fine_granularity() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();

        // "big" exceeds COARSE_MAX_FILES and keeps per-file mirrors; "small" stays below both
        // limits and is recorded as one coarse leaf.
        let mut files: Vec<(String, String)> = (0..20)
            .map(|i| {
                (
                    format!("big/file_{:02}", i),
                    format!("uniqueword{:02} sharedcontent", i),
                )
            })
            .collect();
        files.push(("small/a".to_owned(), "needleinsmall here".to_owned()));
        files.push(("small/b".to_owned(), "other content".to_owned()));
        let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (&p[..], &c[..])).collect();
        let tree = commit_tree(&repo, &files);

        let index =
            trigram_index(&objects(&repo), &cache, &mut Indexer::default(), tree.id()).unwrap();

        // Fine: "d07" (from uniqueword07) mirrors big's structure down to the file's bucket.
        let leaf = path_entry(
            &objects(&repo),
            index,
            std::path::Path::new(&format!(
                "64/30/37/{}/{}",
                bucket_name(b"big"),
                bucket_name(b"file_07")
            )),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            repo.find_object(leaf, None).unwrap().kind(),
            Some(git2::ObjectType::Blob)
        );

        // Coarse: "nee" (from needleinsmall) records small as a blob leaf, not a subtree.
        let leaf = path_entry(
            &objects(&repo),
            index,
            std::path::Path::new(&format!("6e/65/65/{}", bucket_name(b"small"))),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            repo.find_object(leaf, None).unwrap().kind(),
            Some(git2::ObjectType::Blob)
        );

        let mut sc = SearchCache::default();

        // Fine candidates stay per-file (modulo bucket collisions) and matches exact.
        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "uniqueword07").unwrap();
        assert!(candidates.iter().any(|(p, _)| p == "big/file_07"));
        let matches =
            search_matches(&objects(&repo), &mut sc, "uniqueword07", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "big/file_07");

        // A coarse hit makes every file under the directory a candidate; verification is
        // exact.
        let candidates =
            search_candidates(&objects(&repo), &mut sc, index, tree.id(), "needleinsmall").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["small/a", "small/b"]);
        let matches =
            search_matches(&objects(&repo), &mut sc, "needleinsmall", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "small/a");

        // Determinism across memoization states, coarse dirs included.
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
        let index_a = trigram_index(&objects(&repo), &cache, &mut indexer, tree_a.id()).unwrap();

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

        // The roots are memoized in the persistent cache. (The sub dirs are below the coarse
        // threshold and live in the Indexer's coarse memo instead — only fine-grained indexes
        // go through the IndexCache.)
        assert!(cache.get_index(tree_b.id()).is_some());

        // Warm (incremental) and cold-built indexes agree bit for bit.
        let cold = MapCache::default();
        let index_b_cold =
            trigram_index(&objects(&repo), &cold, &mut Indexer::default(), tree_b.id()).unwrap();
        assert_eq!(index_b, index_b_cold);

        // The incremental index searches correctly — through a cache warmed on commit A,
        // like one GraphQL history+search query warms it: memo entries are content-keyed, so
        // cross-commit reuse must not leak commit A's results into commit B's.
        let mut sc = SearchCache::default();
        let hits =
            search_candidates(&objects(&repo), &mut sc, index_a, tree_a.id(), "delta").unwrap();
        assert!(hits.is_empty());
        let hits =
            search_candidates(&objects(&repo), &mut sc, index_a, tree_a.id(), "zebra").unwrap();
        let matches = search_matches(&objects(&repo), &mut sc, "zebra", &hits).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "sub1/gone");

        let hits =
            search_candidates(&objects(&repo), &mut sc, index_b, tree_b.id(), "delta").unwrap();
        assert_eq!(
            hits.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub2/mod"]
        );
        let hits =
            search_candidates(&objects(&repo), &mut sc, index_b, tree_b.id(), "zebra").unwrap();
        assert!(hits.is_empty());
        let hits =
            search_candidates(&objects(&repo), &mut sc, index_b, tree_b.id(), "addition").unwrap();
        assert_eq!(
            hits.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub3/new"]
        );

        // Warm-cache results equal fresh-cache results.
        let fresh = search_candidates(
            &objects(&repo),
            &mut SearchCache::default(),
            index_b,
            tree_b.id(),
            "delta",
        )
        .unwrap();
        assert_eq!(
            fresh.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub2/mod"]
        );
    }
}
