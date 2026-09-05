//! Trigram based code search index for git repositories.
//!
//! The index of a tree is itself a git tree: an inverted index mapping every trigram
//! (3-character window of file content, normalized by [`fold_char`]) to the set of files
//! containing it. Each trigram hashes to a fixed "spine" path of one tree level per
//! [`SPINE_BITS`] entry (see [`spine_path`]), below which a mirror of the source tree's
//! structure, restricted to the files containing that trigram, hangs:
//!
//! ```text
//! <hex(s1)>/<hex(s2)>/<bucket>/<bucket>...
//! ```
//!
//! with the empty blob as the leaf marker. Four lossy steps keep that structure small: the
//! fold merges trigram classes, hashing lets trigrams share a spine leaf, mirror entries are
//! named by a one-byte hash of the source name ([`bucket_name`]) so colliding siblings share
//! a bucket, and small directories are recorded at directory granularity
//! ([`COARSE_MAX_FILES`]). None of them can drop a file from a trigram's set, only add one,
//! so candidates are a superset of the true matches and [`search_matches`] performs exact
//! regular-expression verification.
//!
//! Indexes are built compositionally: each file gets a small trigram tree of its own, child
//! directory indexes are lifted into the parent's namespace, and a directory's children are
//! merged in a single pass. Memoization through [`IndexCache`] (per subtree) and the
//! caller-kept [`Indexer`] state (per blob, wrap and merge) makes reindexing a chain of
//! commits incremental: only what changed is rebuilt. Trees are hashed in memory with
//! [`gix_object`], and only the objects a finished index references are written to the
//! object database.
//!
//! Searching converts a regular expression into a Cox-style Boolean query over required
//! trigrams, evaluates that query against the spine, and verifies the resulting candidates.
//! [`SearchCache`] memoizes each layer, so searching a history reuses shared work.
//!
//! This crate is independent of the josh filter machinery: it operates on plain Git objects and
//! memoizes tree-to-index mappings through the [`IndexCache`] trait the caller provides.

mod query;

use gix_object::WriteTo;
use gix_object::bstr::BString;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Memoization of tree oid -> index tree oid mappings, provided by the caller.
///
/// [`trigram_index`] consults this per (sub)tree, which is what makes indexing incremental: when
/// a new commit is indexed, unchanged subtrees hit the cache and reuse their index.
pub trait IndexCache {
    fn get_index(&self, tree: gix_hash::ObjectId) -> Option<gix_hash::ObjectId>;
    fn set_index(&self, tree: gix_hash::ObjectId, index: gix_hash::ObjectId);
}

fn empty_tree() -> gix_hash::ObjectId {
    gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1)
}

fn empty_blob() -> gix_hash::ObjectId {
    gix_hash::ObjectId::empty_blob(gix_hash::Kind::Sha1)
}

/// ASCII punctuation kept distinct by [`fold_char`] instead of collapsing into the class
/// glyph: brackets, operators and common punctuation carry real structure in code, and
/// keeping them distinct is what makes queries like `->foo(` or `a[i]` selective. Changing
/// this set changes the index format.
const KEEP_DISTINCT: &[u8] = br#"()[]{}<>+-*/=&|!~^%.,;:#@"'\"#;

/// The fold class of every non-ASCII character. Distinct from the whitespace glyph so
/// non-ASCII runs keep their boundary signal (`naïve` stays distinguishable from `na ve`),
/// and free for that purpose because a literal DEL folds to the whitespace glyph. Collapsing
/// per character (not per byte) keeps the folded alphabet pure ASCII, bounding the trigram
/// key space regardless of how much non-ASCII text a corpus contains; the cost is that
/// non-ASCII query text carries no per-character selectivity (verification stays exact).
const NON_ASCII_GLYPH: u8 = 0x7f;

const fn keeps_distinct(b: u8) -> bool {
    let mut i = 0;
    while i < KEEP_DISTINCT.len() {
        if KEEP_DISTINCT[i] == b {
            return true;
        }
        i += 1;
    }
    false
}

/// Fold a character for trigram extraction: ASCII letters lowercase; alphanumerics, `_` and
/// the [`KEEP_DISTINCT`] punctuation stay themselves; every other ASCII character (whitespace
/// and the remaining punctuation) becomes one class glyph, and every non-ASCII character
/// becomes [`NON_ASCII_GLYPH`]. Folding collapses the combinatorial variety of
/// near-content-free trigrams — indentation and line-break variants of the same code —
/// while folded trigrams keep their positional filtering power (a query like `foo(` still
/// requires the exact `(` after `foo`).
const FOLD_TABLE: [u8; 128] = {
    let mut table = [0u8; 128];
    let mut i = 0;
    while i < 128 {
        let b = i as u8;
        table[i] = if b.is_ascii_uppercase() {
            b + 32
        } else if b.is_ascii_alphanumeric() || b == b'_' || keeps_distinct(b) {
            b
        } else {
            b' '
        };
        i += 1;
    }
    table
};

fn fold_char(c: char) -> u8 {
    if c.is_ascii() {
        FOLD_TABLE[c as usize]
    } else {
        NON_ASCII_GLYPH
    }
}

/// All distinct trigrams of `content`: 3-character windows of the [`fold_char`]-normalized
/// text. Index side and query side must fold identically — that is what makes every trigram
/// of a query present in every file containing the query string.
fn distinct_trigrams(content: &str) -> BTreeSet<[u8; 3]> {
    let folded: Vec<u8> = content.chars().map(fold_char).collect();
    folded.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
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

/// Spine geometry: every trigram maps to a path of `SPINE_BITS.len()` tree levels, level
/// `i`'s name being a `SPINE_BITS[i]`-bit slice of [`trigram_hash`], giving a spine of
/// `2^(sum)` buckets with content-independent fan-out. The widths may differ per level to
/// trade node size against node count. The constant is format-defining: changing it changes
/// every index.
const SPINE_BITS: &[u32] = &[6, 6];
const SPINE_LEVELS: usize = SPINE_BITS.len();
const _: () = {
    let mut total = 0;
    let mut i = 0;
    while i < SPINE_BITS.len() {
        assert!(1 <= SPINE_BITS[i] && SPINE_BITS[i] <= 8);
        total += SPINE_BITS[i];
        i += 1;
    }
    assert!(SPINE_LEVELS >= 1 && total <= 64);
};

/// FNV-1a over the folded trigram, finished with one murmur fmix64 step: FNV alone passes
/// its last input byte through a single multiply, too little mixing for the small bit
/// slices [`spine_path`] takes. Format-defining like the geometry constants.
fn trigram_hash(t: [u8; 3]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < 3 {
        h = (h ^ t[i] as u64).wrapping_mul(0x00000100000001b3);
        i += 1;
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^ (h >> 33)
}

/// The spine path of a trigram: one byte of [`SPINE_BITS`]`[i]` bits per level, sliced
/// consecutively from [`trigram_hash`]. Trigrams whose paths collide share a spine leaf,
/// whose mirror is then the union of their file sets.
fn spine_path(t: [u8; 3]) -> [u8; SPINE_LEVELS] {
    let h = trigram_hash(t);
    let mut path = [0u8; SPINE_LEVELS];
    let mut shift = 0;
    for (i, &bits) in SPINE_BITS.iter().enumerate() {
        path[i] = ((h >> shift) & ((1u64 << bits) - 1)) as u8;
        shift += bits;
    }
    path
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
    tree_memo: HashMap<gix_hash::ObjectId, gix_hash::ObjectId>,
    /// Source blob and bucket -> the file's wrapped trigram tree. The bucket is part of the
    /// key because the mirror entries inside carry it.
    blob_memo: HashMap<(gix_hash::ObjectId, String), gix_hash::ObjectId>,
    /// Source blob -> its nameless trigram spine (empty-blob leaves), the building block of
    /// coarse indexes. Name-independent, so identical blobs share it everywhere.
    blob_spine_memo: HashMap<gix_hash::ObjectId, gix_hash::ObjectId>,
    /// Source tree -> its coarse index (the subtree treated as one pseudo-file). Kept apart
    /// from the fine-grained [`IndexCache`]; coarse subtrees are small by definition, so
    /// keeping this per process is cheap enough.
    coarse_memo: HashMap<gix_hash::ObjectId, gix_hash::ObjectId>,
    /// Source tree -> transitive `(file count, content bytes)`, for the granularity decision.
    stats_memo: HashMap<gix_hash::ObjectId, (u64, u64)>,
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
    roots: Vec<(gix_hash::ObjectId, gix_hash::ObjectId)>,
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
    /// [`spine_path`] leaf, or the empty tree when there are no trigrams.
    fn content_spine(
        &mut self,
        content: &str,
        leaf_mode: gix_object::tree::EntryMode,
        leaf_oid: gix_hash::ObjectId,
    ) -> gix_hash::ObjectId {
        // The set dedups colliding trigrams (same path, identical leaf here) and keeps the
        // paths in hex order, so write_tree's canonical sort is a no-op below.
        let paths: BTreeSet<[u8; SPINE_LEVELS]> = distinct_trigrams(content)
            .iter()
            .map(|t| spine_path(*t))
            .collect();
        if paths.is_empty() {
            return empty_tree();
        }
        let paths: Vec<_> = paths.into_iter().collect();
        self.spine_subtree(&paths, 0, leaf_mode, leaf_oid)
    }

    /// One spine level: group the sorted, deduplicated `paths` by their byte at `depth`,
    /// recursing per group and emitting the leaf entries at the last level.
    fn spine_subtree(
        &mut self,
        paths: &[[u8; SPINE_LEVELS]],
        depth: usize,
        leaf_mode: gix_object::tree::EntryMode,
        leaf_oid: gix_hash::ObjectId,
    ) -> gix_hash::ObjectId {
        let mut entries = Vec::new();
        let mut i = 0;
        while i < paths.len() {
            let byte = paths[i][depth];
            let mut j = i + 1;
            while j < paths.len() && paths[j][depth] == byte {
                j += 1;
            }
            if depth + 1 == SPINE_LEVELS {
                entries.push(gix_object::tree::Entry {
                    mode: leaf_mode,
                    filename: hex_name(byte),
                    oid: leaf_oid,
                });
            } else {
                let child = self.spine_subtree(&paths[i..j], depth + 1, leaf_mode, leaf_oid);
                entries.push(tree_entry(hex_name(byte), child));
            }
            i = j;
        }
        self.write_tree(entries)
    }

    /// The wrapped trigram tree of one file: its trigram spine with the one-entry mirror
    /// `{bucket: empty blob}` at every leaf. Building the wrapped form directly (rather than
    /// wrapping afterwards via [`wrap`](Run::wrap)) avoids an intermediate tree that would
    /// be rewritten anyway.
    fn index_blob(
        &mut self,
        oid: gix_hash::ObjectId,
        bucket: &str,
        content: &str,
    ) -> gix_hash::ObjectId {
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
    fn blob_spine(&mut self, oid: gix_hash::ObjectId, content: &str) -> gix_hash::ObjectId {
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
    fn subtree_stats(&mut self, tree_oid: gix_hash::ObjectId) -> anyhow::Result<(u64, u64)> {
        if let Some(stats) = self.ix.stats_memo.get(&tree_oid) {
            return Ok(*stats);
        }
        let tree = self.read_tree(tree_oid)?;
        let (mut files, mut bytes) = (0u64, 0u64);
        for entry in tree.entries.clone() {
            if entry.mode.is_tree() {
                let (f, b) = self.subtree_stats(entry.oid.to_owned())?;
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
    fn is_small(&mut self, tree_oid: gix_hash::ObjectId) -> anyhow::Result<bool> {
        let (files, bytes) = self.subtree_stats(tree_oid)?;
        Ok(files <= COARSE_MAX_FILES && bytes <= COARSE_MAX_BYTES)
    }

    /// The coarse index of a tree: the whole subtree treated as one pseudo-file — the spine of
    /// every trigram occurring anywhere below it, with empty-blob leaves and no mirror
    /// structure. Wrapping it under the directory's name yields `{name: empty blob}` mirrors:
    /// a blob leaf where the source has a directory, meaning "some file under here contains
    /// the trigram".
    fn coarse_index(&mut self, tree_oid: gix_hash::ObjectId) -> anyhow::Result<gix_hash::ObjectId> {
        if let Some(id) = self.ix.coarse_memo.get(&tree_oid) {
            return Ok(*id);
        }

        let tree = self.read_tree(tree_oid)?;
        let mut spines = Vec::with_capacity(tree.entries.len());
        for entry in tree.entries.clone() {
            if entry.mode.is_tree() {
                spines.push(self.coarse_index(entry.oid.to_owned())?);
            } else if !entry.mode.is_commit() {
                let content = read_blob_text(self.src, entry.oid);
                spines.push(self.blob_spine(entry.oid.to_owned(), &content));
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

        let id = self.wrap_level(bucket, index, SPINE_LEVELS)?;
        self.ix.wrap_memo.insert(key, id);
        Ok(id)
    }

    /// Rewrite one spine level for [`wrap`](Run::wrap): with `remaining` levels left the
    /// entries are spine nodes to recurse into; at the last level they are the mirrors (or
    /// coarse leaves) to nest under `bucket`.
    fn wrap_level(
        &mut self,
        bucket: &str,
        node: gix_hash::ObjectId,
        remaining: usize,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let tree = self.read_tree(node)?;
        let mut entries = Vec::with_capacity(tree.entries.len());
        for e in tree.entries.iter() {
            let child = if remaining == 1 {
                self.write_tree(vec![gix_object::tree::Entry {
                    mode: e.mode,
                    filename: bucket.into(),
                    oid: e.oid,
                }])
            } else {
                self.wrap_level(bucket, e.oid, remaining - 1)?
            };
            entries.push(tree_entry(e.filename.clone(), child));
        }
        Ok(self.write_tree(entries))
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
    fn index_tree_oid(
        &mut self,
        tree_oid: gix_hash::ObjectId,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        let tree = self.read_tree(tree_oid)?;
        if let Some(id) = self.ix.tree_memo.get(&tree_oid) {
            return Ok(*id);
        }
        if let Some(cached) = self.cache.get_index(tree_oid) {
            let id = cached;
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
                self.is_small(entry.oid.to_owned())?
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
                let child_oid = entry.oid.to_owned();
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
                wrapped.push(self.index_blob(entry.oid.to_owned(), bucket, &content));
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
            self.cache.set_index(*tree, *index);
        }
        Ok(())
    }
}

pub fn trigram_index(
    src: &dyn Objects,
    cache: &dyn IndexCache,
    indexer: &mut Indexer,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut run = Run {
        src,
        cache,
        ix: indexer,
        roots: Vec::new(),
    };
    let index = run.index_tree_oid(tree)?;
    run.flush()?;
    Ok(index)
}

/// Search memoization, meant to be kept alive for many [`search_candidates`] /
/// [`search_matches`] calls (josh keeps one per transaction). Everything is keyed by content
/// — object ids and the regex pattern — so entries are valid for any commit of the repository:
/// searching a history reuses the candidate walks of every shared subtree and verifies every
/// distinct blob once, no matter how many commits it appears in.
#[derive(Default)]
pub struct SearchCache {
    /// Pattern -> compiled matcher and conservative trigram query.
    plans: HashMap<String, std::sync::Arc<query::Plan>>,
    /// (pattern, index tree, source tree) -> sorted candidate (path, blob) pairs.
    candidates: HashMap<
        (String, gix_hash::ObjectId, gix_hash::ObjectId),
        std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>,
    >,
    /// (mirror roots (normalized), source tree) -> relative candidate (path, blob) pairs.
    walks: HashMap<
        (Vec<gix_hash::ObjectId>, gix_hash::ObjectId),
        std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>,
    >,
    /// Mirror tree oid -> its entries in compact parsed form. Mirrors are shared heavily
    /// across commits and trigrams, and parsing each distinct mirror once avoids repeated
    /// object decoding.
    mirrors: HashMap<gix_hash::ObjectId, std::sync::Arc<Vec<MirrorEntry>>>,
    /// source tree -> all (relative path, blob) pairs under it.
    all_paths: HashMap<gix_hash::ObjectId, std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>>,
    /// (pattern, blob) -> matching (line number, line) pairs.
    blob_matches: HashMap<(String, gix_hash::ObjectId), std::sync::Arc<Vec<(usize, String)>>>,
}

fn regex_plan(
    cache: &mut SearchCache,
    pattern: &str,
) -> anyhow::Result<std::sync::Arc<query::Plan>> {
    if let Some(plan) = cache.plans.get(pattern) {
        return Ok(plan.clone());
    }
    let plan = std::sync::Arc::new(query::Plan::new(pattern)?);
    cache.plans.insert(pattern.to_owned(), plan.clone());
    Ok(plan)
}

/// The candidate files for `pattern`: a conservative superset selected by the Cox-style
/// Boolean trigram query extracted from the regular expression.
///
/// Patterns without a required trigram admit every file of `source_tree`; [`search_matches`]
/// performs exact regex verification.
pub fn search_candidates(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: gix_hash::ObjectId,
    source_tree: gix_hash::ObjectId,
    pattern: &str,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let plan = regex_plan(cache, pattern)?;
    search_candidates_with_plan(src, cache, index_tree, source_tree, pattern, &plan)
}

fn search_candidates_with_plan(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: gix_hash::ObjectId,
    source_tree: gix_hash::ObjectId,
    pattern: &str,
    plan: &query::Plan,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let key = (pattern.to_owned(), index_tree, source_tree);
    if let Some(hit) = cache.candidates.get(&key) {
        return Ok((**hit).clone());
    }

    let results = (*evaluate_query(src, cache, index_tree, source_tree, &plan.query)?).clone();
    cache
        .candidates
        .insert(key, std::sync::Arc::new(results.clone()));
    Ok(results)
}

/// The normalized mirror roots for trigrams required in every branch of `pattern`.
///
/// This compatibility view deliberately omits branch-local OR constraints. `None` means the
/// regex has no globally required trigrams (every file is a possible match); an empty vector
/// means the regex cannot match or a required trigram is absent. [`search_candidates`] evaluates
/// the complete Boolean query, including alternation.
pub fn query_roots(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: gix_hash::ObjectId,
    pattern: &str,
) -> anyhow::Result<Option<Vec<gix_hash::ObjectId>>> {
    let plan = regex_plan(cache, pattern)?;
    if plan.query == query::Query::None {
        return Ok(Some(vec![]));
    }
    let trigrams = plan.query.required_trigrams();
    if trigrams.is_empty() {
        return Ok(None);
    }

    let mut roots = vec![];
    for trigram in trigrams {
        let Some(root) = resolve_trigram_root(src, cache, index_tree, trigram)? else {
            return Ok(Some(vec![]));
        };
        roots.push(root);
    }
    normalize_roots(src, cache, &mut roots)?;
    Ok(Some(roots))
}

fn resolve_trigram_root(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: gix_hash::ObjectId,
    trigram: [u8; 3],
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let mut node = index_tree;
    for byte in spine_path(trigram) {
        let entries = mirror_entries(src, cache, node)?;
        let Ok(index) = entries.binary_search_by_key(&hex_pair(byte), |entry| entry.name) else {
            return Ok(None);
        };
        node = entries[index].oid;
    }
    Ok(Some(node))
}

fn normalize_roots(
    src: &dyn Objects,
    cache: &mut SearchCache,
    roots: &mut Vec<gix_hash::ObjectId>,
) -> anyhow::Result<()> {
    roots.sort();
    roots.dedup();

    const MAX_INTERSECT: usize = 16;
    if roots.len() > MAX_INTERSECT {
        let mut sized = roots
            .iter()
            .map(|&oid| anyhow::Ok((mirror_entries(src, cache, oid)?.len(), oid)))
            .collect::<Result<Vec<_>, _>>()?;
        sized.sort();
        *roots = sized
            .into_iter()
            .take(MAX_INTERSECT)
            .map(|(_, oid)| oid)
            .collect();
    }
    Ok(())
}

fn candidates_for_trigrams(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: gix_hash::ObjectId,
    source_tree: gix_hash::ObjectId,
    trigrams: impl IntoIterator<Item = [u8; 3]>,
) -> anyhow::Result<std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>> {
    let mut roots = vec![];
    for trigram in trigrams {
        let Some(root) = resolve_trigram_root(src, cache, index_tree, trigram)? else {
            return Ok(std::sync::Arc::new(vec![]));
        };
        roots.push(root);
    }
    normalize_roots(src, cache, &mut roots)?;
    if roots.is_empty() {
        all_paths(src, cache, source_tree)
    } else {
        walk(src, cache, roots, source_tree)
    }
}

fn evaluate_query(
    src: &dyn Objects,
    cache: &mut SearchCache,
    index_tree: gix_hash::ObjectId,
    source_tree: gix_hash::ObjectId,
    query: &query::Query,
) -> anyhow::Result<std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>> {
    match query {
        query::Query::All => all_paths(src, cache, source_tree),
        query::Query::None => Ok(std::sync::Arc::new(vec![])),
        query::Query::Trigram(trigram) => {
            candidates_for_trigrams(src, cache, index_tree, source_tree, [*trigram])
        }
        query::Query::And(children) => {
            let direct: Vec<_> = children
                .iter()
                .filter_map(|child| match child {
                    query::Query::Trigram(trigram) => Some(*trigram),
                    _ => None,
                })
                .collect();
            let mut result = if direct.is_empty() {
                None
            } else {
                Some(candidates_for_trigrams(
                    src,
                    cache,
                    index_tree,
                    source_tree,
                    direct,
                )?)
            };
            for child in children {
                if matches!(child, query::Query::Trigram(_)) {
                    continue;
                }
                let candidates = evaluate_query(src, cache, index_tree, source_tree, child)?;
                result = Some(match result {
                    None => candidates,
                    Some(current) => {
                        std::sync::Arc::new(intersect_candidates(&current, &candidates))
                    }
                });
                if result.as_ref().unwrap().is_empty() {
                    break;
                }
            }
            match result {
                Some(result) => Ok(result),
                None => all_paths(src, cache, source_tree),
            }
        }
        query::Query::Or(children) => {
            let mut result = vec![];
            for child in children {
                let candidates = evaluate_query(src, cache, index_tree, source_tree, child)?;
                result.extend(candidates.iter().cloned());
            }
            result.sort();
            result.dedup();
            Ok(std::sync::Arc::new(result))
        }
    }
}

fn intersect_candidates(
    left: &[(String, gix_hash::ObjectId)],
    right: &[(String, gix_hash::ObjectId)],
) -> Vec<(String, gix_hash::ObjectId)> {
    let (mut left_index, mut right_index) = (0, 0);
    let mut out = Vec::with_capacity(left.len().min(right.len()));
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                out.push(left[left_index].clone());
                left_index += 1;
                right_index += 1;
            }
        }
    }
    out
}

/// One mirror tree entry in compact parsed form: the fixed-width bucket name, the child oid,
/// and whether it is a subtree (versus a coarse or file leaf).
struct MirrorEntry {
    name: [u8; 2],
    oid: gix_hash::ObjectId,
    tree: bool,
}

/// The entries of the mirror tree `oid`, parsed once per process and memoized.
fn mirror_entries(
    src: &dyn Objects,
    cache: &mut SearchCache,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<std::sync::Arc<Vec<MirrorEntry>>> {
    if let Some(hit) = cache.mirrors.get(&oid) {
        return Ok(hit.clone());
    }
    let mut entries = vec![];
    for entry in read_tree_entries(src, oid)? {
        if let [a, b] = entry.filename.as_slice() {
            entries.push(MirrorEntry {
                name: [*a, *b],
                oid: entry.oid.to_owned(),
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
    mut roots: Vec<gix_hash::ObjectId>,
    source: gix_hash::ObjectId,
) -> anyhow::Result<std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>> {
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
                let child_roots: Vec<gix_hash::ObjectId> =
                    (0..k).map(|i| entry_lists[i][idx[i]].oid).collect();
                for member in members {
                    if !member.mode.is_tree() {
                        continue;
                    }
                    let name = std::str::from_utf8(&member.filename)?;
                    let sub = walk(src, cache, child_roots.clone(), member.oid.to_owned())?;
                    out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
                }
            } else {
                for member in members {
                    let name = std::str::from_utf8(&member.filename)?;
                    if member.mode.is_tree() {
                        let sub = all_paths(src, cache, member.oid.to_owned())?;
                        out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
                    } else if !member.mode.is_commit() {
                        out.push((name.to_owned(), member.oid.to_owned()));
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
    oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<gix_object::tree::Entry>> {
    let mut buffer = Vec::new();
    let data = src
        .try_find(&oid, &mut buffer)
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
    oid: gix_hash::ObjectId,
) -> anyhow::Result<std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>> {
    if let Some(hit) = cache.all_paths.get(&oid) {
        return Ok(hit.clone());
    }
    let mut out = vec![];
    for entry in read_tree_entries(src, oid)? {
        let name = std::str::from_utf8(&entry.filename)?;
        if entry.mode.is_tree() {
            let sub = all_paths(src, cache, entry.oid.to_owned())?;
            out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
        } else if !entry.mode.is_commit() {
            out.push((name.to_owned(), entry.oid.to_owned()));
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

/// Verify `candidates` against the regular expression, line by line. Per-blob results are
/// memoized on (pattern, blob oid): a blob's matching lines do not depend on the commit or path
/// it appears under, so verifying a history costs one scan per distinct blob. Candidates carry
/// their blob oids from candidate selection, so no path resolution happens here.
pub fn search_matches(
    src: &dyn Objects,
    cache: &mut SearchCache,
    pattern: &str,
    candidates: &[(String, gix_hash::ObjectId)],
) -> anyhow::Result<SearchMatchesResult> {
    let plan = regex_plan(cache, pattern)?;
    search_matches_with_plan(src, cache, pattern, &plan, candidates)
}

fn search_matches_with_plan(
    src: &dyn Objects,
    cache: &mut SearchCache,
    pattern: &str,
    plan: &query::Plan,
    candidates: &[(String, gix_hash::ObjectId)],
) -> anyhow::Result<SearchMatchesResult> {
    let mut results = vec![];

    for (candidate, blob) in candidates {
        let key = (pattern.to_owned(), *blob);
        let blob_results = if let Some(hit) = cache.blob_matches.get(&key) {
            hit.clone()
        } else {
            let content = read_blob_text(src, *blob);
            let lines: Vec<(usize, String)> = content
                .lines()
                .enumerate()
                .filter(|(_, line)| plan.regex.is_match(line))
                .map(|(line_number, line)| (line_number + 1, line.to_owned()))
                .collect();
            let lines = std::sync::Arc::new(lines);
            cache.blob_matches.insert(key, lines.clone());
            lines
        };

        if !blob_results.is_empty() {
            results.push((candidate.to_owned(), (*blob_results).clone()));
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
    tree: gix_hash::ObjectId,
    path: &std::path::Path,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let mut current = tree;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let mut buffer = Vec::new();
        let Some(data) = src
            .try_find(&current, &mut buffer)
            .map_err(|e| anyhow::anyhow!("read tree {}: {}", current, e))?
        else {
            return Ok(None);
        };
        if data.kind != gix_object::Kind::Tree {
            return Ok(None);
        }
        let parsed = gix_object::TreeRef::from_bytes(&buffer, gix_hash::Kind::Sha1)?;
        let name = josh_gix_ext::component_bytes(component.as_os_str());
        let Some(entry) = parsed.entries.iter().find(|e| e.filename == name) else {
            return Ok(None);
        };
        current = entry.oid.to_owned();
        if components.peek().is_some() && !entry.mode.is_tree() {
            return Ok(None);
        }
    }
    Ok(Some(current))
}

/// The number of lines of blob `oid` matching `plan`, through the per-blob match memo.
fn match_count(
    src: &dyn Objects,
    cache: &mut SearchCache,
    pattern: &str,
    plan: &query::Plan,
    blob: gix_hash::ObjectId,
) -> anyhow::Result<usize> {
    let matches = search_matches_with_plan(src, cache, pattern, plan, &[(String::new(), blob)])?;
    Ok(matches.first().map(|result| result.1.len()).unwrap_or(0))
}

/// One match-set change reported by [`ChangeSweep`]: the file's matching-line count went from
/// `before` (in the commit's first parent) to `after`.
pub struct ChangeEvent {
    pub path: String,
    pub before: usize,
    pub after: usize,
}

/// Which parts of the candidate space changed between two indexes, as a trie over bucket
/// bytes. `changed` marks a whole subtree as needing recomputation.
#[derive(Default)]
struct BucketTrie {
    changed: bool,
    children: HashMap<u8, BucketTrie>,
}

/// Union the differences between two mirrors into `node`: entries present on one side only or
/// changing kind mark their bucket changed; tree pairs recurse.
fn diff_mirrors(
    src: &dyn Objects,
    cache: &mut SearchCache,
    a: gix_hash::ObjectId,
    b: gix_hash::ObjectId,
    node: &mut BucketTrie,
) -> anyhow::Result<()> {
    if a == b {
        return Ok(());
    }
    let la = mirror_entries(src, cache, a)?;
    let lb = mirror_entries(src, cache, b)?;
    let (mut i, mut j) = (0, 0);
    while i < la.len() || j < lb.len() {
        let (name, pair) = if j >= lb.len() || (i < la.len() && la[i].name < lb[j].name) {
            let e = &la[i];
            i += 1;
            (e.name, None)
        } else if i >= la.len() || lb[j].name < la[i].name {
            let e = &lb[j];
            j += 1;
            (e.name, None)
        } else {
            let (ea, eb) = (&la[i], &lb[j]);
            i += 1;
            j += 1;
            if ea.oid == eb.oid && ea.tree == eb.tree {
                continue;
            }
            (ea.name, (ea.tree && eb.tree).then_some((ea.oid, eb.oid)))
        };
        let Some(bucket) = parse_bucket(&name) else {
            continue;
        };
        let child = node.children.entry(bucket).or_default();
        match pair {
            Some((ea, eb)) => diff_mirrors(src, cache, ea, eb, child)?,
            None => child.changed = true,
        }
    }
    Ok(())
}

/// Whether a candidate path lies in the changed part of the trie. Conservative: any trie
/// presence at the path's final component counts as changed (regeneration is cheap and only
/// covers trie scope anyway).
fn path_changed(trie: &BucketTrie, path: &str) -> bool {
    let mut node = trie;
    for comp in path.split('/') {
        if node.changed {
            return true;
        }
        match node.children.get(&bucket_byte(comp.as_bytes())) {
            None => return false,
            Some(next) => node = next,
        }
    }
    node.changed || !node.children.is_empty()
}

/// The candidates within the trie's changed scope: like [`walk`], but visiting only buckets
/// the trie names, and delegating fully-changed subtrees to the memoized full walk.
fn walk_restricted(
    src: &dyn Objects,
    cache: &mut SearchCache,
    mut roots: Vec<gix_hash::ObjectId>,
    source: gix_hash::ObjectId,
    trie: &BucketTrie,
    out: &mut Vec<(String, gix_hash::ObjectId)>,
) -> anyhow::Result<()> {
    roots.sort();
    roots.dedup();
    if trie.changed {
        out.extend(walk(src, cache, roots, source)?.iter().cloned());
        return Ok(());
    }

    let entry_lists = roots
        .iter()
        .map(|oid| mirror_entries(src, cache, *oid))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let source_entries = read_tree_entries(src, source)?;
    let mut by_bucket: Vec<Vec<&gix_object::tree::Entry>> = (0..256).map(|_| Vec::new()).collect();
    for entry in &source_entries {
        if !entry.mode.is_commit() {
            by_bucket[bucket_byte(&entry.filename) as usize].push(entry);
        }
    }

    'bucket: for (&bucket, sub) in &trie.children {
        let name = hex_pair(bucket);
        let mut child_roots = Vec::with_capacity(entry_lists.len());
        let mut is_tree = None;
        for list in &entry_lists {
            match list.binary_search_by_key(&name, |e| e.name) {
                // Bucket absent from one mirror: nothing under it can match anymore.
                Err(_) => continue 'bucket,
                Ok(i) => {
                    let e = &list[i];
                    if *is_tree.get_or_insert(e.tree) != e.tree {
                        continue 'bucket;
                    }
                    child_roots.push(e.oid);
                }
            }
        }
        let members = &by_bucket[bucket as usize];
        if is_tree == Some(true) {
            for member in members {
                if !member.mode.is_tree() {
                    continue;
                }
                let mname = std::str::from_utf8(&member.filename)?;
                let mut sub_out = vec![];
                walk_restricted(
                    src,
                    cache,
                    child_roots.clone(),
                    member.oid.to_owned(),
                    sub,
                    &mut sub_out,
                )?;
                out.extend(sub_out.into_iter().map(|(p, b)| (join_path(mname, &p), b)));
            }
        } else {
            for member in members {
                let mname = std::str::from_utf8(&member.filename)?;
                if member.mode.is_tree() {
                    let sub_paths = all_paths(src, cache, member.oid.to_owned())?;
                    out.extend(sub_paths.iter().map(|(p, b)| (join_path(mname, p), *b)));
                } else if !member.mode.is_commit() {
                    out.push((mname.to_owned(), member.oid.to_owned()));
                }
            }
        }
    }
    Ok(())
}

/// Re-resolve `cands`' blob oids in the child tree `c_oid`, descending only where a component
/// subtree differs from the parent's `p_oid`. Candidates are sorted by path, so paths sharing
/// a directory are contiguous: each changed directory is parsed once, identical subtrees are
/// skipped by oid without parsing anything. `false` means a candidate path no longer resolves
/// (e.g. a rename within one hash bucket); the caller falls back to a full walk.
fn refresh_group(
    src: &dyn Objects,
    c_oid: gix_hash::ObjectId,
    p_oid: gix_hash::ObjectId,
    cands: &[(String, gix_hash::ObjectId)],
    depth: usize,
    out: &mut Vec<(String, gix_hash::ObjectId)>,
) -> anyhow::Result<bool> {
    if c_oid == p_oid {
        out.extend_from_slice(cands);
        return Ok(true);
    }
    let c_entries = read_tree_entries(src, c_oid)?;
    let p_entries = read_tree_entries(src, p_oid)?;

    fn component(path: &str, depth: usize) -> &str {
        path.split('/').nth(depth).unwrap_or("")
    }
    let mut i = 0;
    while i < cands.len() {
        let comp = component(&cands[i].0, depth);
        let mut j = i + 1;
        while j < cands.len() && component(&cands[j].0, depth) == comp {
            j += 1;
        }

        let find = |entries: &[gix_object::tree::Entry]| {
            entries
                .iter()
                .find(|e| e.filename == comp.as_bytes())
                .map(|e| e.oid.to_owned())
        };
        let ids = match (find(&c_entries), find(&p_entries)) {
            (Some(ce), Some(pe)) => (ce, pe),
            _ => return Ok(false),
        };
        let is_leaf = cands[i].0.split('/').count() == depth + 1;
        if is_leaf {
            out.push((cands[i].0.clone(), ids.0));
        } else if !refresh_group(src, ids.0, ids.1, &cands[i..j], depth + 1, out)? {
            return Ok(false);
        }
        i = j;
    }
    Ok(true)
}

/// Per-commit sweep state: the source tree, each query trigram's mirror root aligned with the
/// query's spine paths (`None` when that path is absent), and the candidate pairs.
struct SweepState {
    tree: gix_hash::ObjectId,
    roots: Vec<Option<gix_hash::ObjectId>>,
    cands: std::sync::Arc<Vec<(String, gix_hash::ObjectId)>>,
}

/// Pickaxe-style change detection over a history: feed commits parents-first through
/// [`process`](ChangeSweep::process) and collect, per non-merge commit, the files whose
/// matching-line count changed against the first parent.
///
/// Content addressing keeps the per-commit cost proportional to what changed. Equal roots for
/// every trigram prove the Boolean candidate query unchanged, so only blob ids are refreshed.
/// Pure conjunctions also retain the restricted mirror diff; queries containing alternation
/// fall back to evaluating the cached Boolean query when their roots change.
pub struct ChangeSweep {
    pattern: String,
    plan: std::sync::Arc<query::Plan>,
    /// Every distinct trigram spine path used anywhere in the Boolean query. Colliding
    /// trigrams collapse because they resolve to the same mirror root.
    spine_paths: BTreeSet<[u8; SPINE_LEVELS]>,
    conjunctive: bool,
    store: HashMap<gix_hash::ObjectId, SweepState>,
}

impl ChangeSweep {
    pub fn new(pattern: &str) -> anyhow::Result<Self> {
        let plan = std::sync::Arc::new(query::Plan::new(pattern)?);
        let mut trigrams = BTreeSet::new();
        plan.query.trigrams(&mut trigrams);
        Ok(Self {
            pattern: pattern.to_owned(),
            spine_paths: trigrams.into_iter().map(spine_path).collect(),
            conjunctive: plan.query.conjunctive_trigrams().is_some(),
            plan,
            store: HashMap::new(),
        })
    }

    /// Each query trigram's mirror root, aligned with `self.spine_paths`.
    fn trigram_roots(
        &self,
        src: &dyn Objects,
        cache: &mut SearchCache,
        index_oid: gix_hash::ObjectId,
    ) -> anyhow::Result<Vec<Option<gix_hash::ObjectId>>> {
        let mut roots = Vec::with_capacity(self.spine_paths.len());
        for path in &self.spine_paths {
            let mut node = Some(index_oid);
            for byte in path {
                let Some(oid) = node else {
                    break;
                };
                let entries = mirror_entries(src, cache, oid)?;
                node = entries
                    .binary_search_by_key(&hex_pair(*byte), |entry| entry.name)
                    .ok()
                    .map(|index| entries[index].oid);
            }
            roots.push(node);
        }
        Ok(roots)
    }

    pub fn process(
        &mut self,
        src: &dyn Objects,
        cache: &mut SearchCache,
        commit_id: gix_hash::ObjectId,
        parent_ids: &[gix_hash::ObjectId],
        source_tree: gix_hash::ObjectId,
        index_tree: gix_hash::ObjectId,
    ) -> anyhow::Result<Vec<ChangeEvent>> {
        let roots = self.trigram_roots(src, cache, index_tree)?;
        let parent = parent_ids.first().and_then(|parent| self.store.get(parent));

        let cands = 'cands: {
            if let Some(parent_state) = parent {
                if parent_state.tree == source_tree {
                    break 'cands parent_state.cands.clone();
                }
                if parent_state.roots == roots {
                    // Identical roots: same candidate paths, only blobs may differ.
                    let mut out = Vec::with_capacity(parent_state.cands.len());
                    if refresh_group(
                        src,
                        source_tree,
                        parent_state.tree,
                        &parent_state.cands,
                        0,
                        &mut out,
                    )? {
                        break 'cands std::sync::Arc::new(out);
                    }
                } else if self.conjunctive {
                    let current: Option<Vec<_>> = roots.iter().copied().collect();
                    let previous: Option<Vec<_>> = parent_state.roots.iter().copied().collect();
                    if let (Some(current), Some(previous)) = (current, previous) {
                        // Diff the changed mirrors into a trie, splice: keep and re-resolve
                        // the parent's candidates outside the trie, re-walk only inside it.
                        let mut trie = BucketTrie::default();
                        for (current, previous) in current.iter().zip(&previous) {
                            diff_mirrors(src, cache, *current, *previous, &mut trie)?;
                        }
                        let kept: Vec<(String, gix_hash::ObjectId)> = parent_state
                            .cands
                            .iter()
                            .filter(|(path, _)| !path_changed(&trie, path))
                            .cloned()
                            .collect();
                        let mut out = Vec::with_capacity(kept.len());
                        if refresh_group(src, source_tree, parent_state.tree, &kept, 0, &mut out)? {
                            walk_restricted(src, cache, current, source_tree, &trie, &mut out)?;
                            out.sort();
                            out.dedup();
                            break 'cands std::sync::Arc::new(out);
                        }
                    }
                }
            }
            std::sync::Arc::new(search_candidates_with_plan(
                src,
                cache,
                index_tree,
                source_tree,
                &self.pattern,
                &self.plan,
            )?)
        };

        // Change events only for non-merge commits, like git log -S without diff-merges.
        let mut events = vec![];
        if parent_ids.len() <= 1 {
            let empty = std::sync::Arc::new(vec![]);
            let pcands = parent_ids
                .first()
                .and_then(|p| self.store.get(p))
                .map(|s| s.cands.clone())
                .unwrap_or(empty);
            events = self.diff_events(src, cache, &pcands, &cands)?;
        }

        self.store.insert(
            commit_id,
            SweepState {
                tree: source_tree,
                roots,
                cands,
            },
        );
        Ok(events)
    }

    fn diff_events(
        &self,
        src: &dyn Objects,
        cache: &mut SearchCache,
        pcands: &[(String, gix_hash::ObjectId)],
        cands: &[(String, gix_hash::ObjectId)],
    ) -> anyhow::Result<Vec<ChangeEvent>> {
        let mut events = vec![];
        let (mut i, mut j) = (0, 0);
        while i < cands.len() || j < pcands.len() {
            let (path, before, after) =
                if j >= pcands.len() || (i < cands.len() && cands[i].0 < pcands[j].0) {
                    let (path, blob) = &cands[i];
                    i += 1;
                    (
                        path,
                        0,
                        match_count(src, cache, &self.pattern, &self.plan, *blob)?,
                    )
                } else if i >= cands.len() || pcands[j].0 < cands[i].0 {
                    let (path, blob) = &pcands[j];
                    j += 1;
                    (
                        path,
                        match_count(src, cache, &self.pattern, &self.plan, *blob)?,
                        0,
                    )
                } else {
                    let (path, blob) = &cands[i];
                    let pblob = pcands[j].1;
                    i += 1;
                    j += 1;
                    if *blob == pblob {
                        continue;
                    }
                    (
                        path,
                        match_count(src, cache, &self.pattern, &self.plan, pblob)?,
                        match_count(src, cache, &self.pattern, &self.plan, *blob)?,
                    )
                };
            if before != after {
                events.push(ChangeEvent {
                    path: path.clone(),
                    before,
                    after,
                });
            }
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_repo() -> (tempfile::TempDir, gix::Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(tmp.path()).unwrap();
        (tmp, repo)
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
        // Windows are per character, and every non-ASCII character is one glyph: "aéb" is a
        // single trigram, and all non-ASCII characters fold together.
        assert_eq!(
            distinct_trigrams("aéb"),
            BTreeSet::from([[b'a', NON_ASCII_GLYPH, b'b']])
        );
        assert_eq!(distinct_trigrams("aéb"), distinct_trigrams("a\u{4e2d}b"));
        // ... but distinct from the whitespace glyph, keeping boundary signal.
        assert_ne!(distinct_trigrams("aéb"), distinct_trigrams("a b"));
        // Case folds; whitespace and uncurated punctuation are one class glyph; word bytes
        // ([a-z0-9_]) and the KEEP_DISTINCT punctuation stay distinct.
        assert_eq!(distinct_trigrams("AbC"), distinct_trigrams("abc"));
        assert_eq!(distinct_trigrams("a b"), distinct_trigrams("a\tb"));
        assert_eq!(distinct_trigrams("a?b"), distinct_trigrams("a b"));
        assert_ne!(distinct_trigrams("a,b"), distinct_trigrams("a;b"));
        assert_ne!(distinct_trigrams("f(x)"), distinct_trigrams("f[x]"));
        assert_ne!(distinct_trigrams("a_b"), distinct_trigrams("a b"));
        assert_ne!(distinct_trigrams("a1b"), distinct_trigrams("a2b"));
    }

    #[test]
    fn fold_policy() {
        // Curated punctuation stays itself, uncurated ASCII collapses to the glyph.
        for &b in KEEP_DISTINCT {
            assert_eq!(fold_char(b as char), b);
        }
        for c in [' ', '\t', '\n', '\r', '?', '$', '`', '\u{0}', '\u{7f}'] {
            assert_eq!(fold_char(c), b' ');
        }
        assert_eq!(fold_char('A'), b'a');
        assert_eq!(fold_char('z'), b'z');
        assert_eq!(fold_char('7'), b'7');
        assert_eq!(fold_char('_'), b'_');
        // Every non-ASCII character folds to the one non-ASCII glyph.
        assert_eq!(fold_char('é'), NON_ASCII_GLYPH);
        assert_eq!(fold_char('\u{4e2d}'), NON_ASCII_GLYPH);
        assert_eq!(fold_char('\u{1f600}'), NON_ASCII_GLYPH);
    }

    #[test]
    fn spine_path_stability() {
        // The trigram -> spine path mapping is format-defining: cached indexes survive only
        // as long as these paths do not move.
        for path in [
            spine_path(*b"tes"),
            spine_path(*b"a b"),
            spine_path([b'a', NON_ASCII_GLYPH, b'b']),
        ] {
            for (b, bits) in path.iter().zip(SPINE_BITS) {
                assert!(*b < (1u32 << bits) as u8);
            }
        }
        assert_eq!(spine_dir(*b"tes"), "0d/07");
        assert_eq!(spine_dir(*b"nee"), "23/00");
        assert_eq!(spine_dir(*b"d07"), "23/29");
    }

    /// A trigram's spine path as the index tree path string.
    fn spine_dir(t: [u8; 3]) -> String {
        spine_path(t)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join("/")
    }

    #[derive(Default)]
    struct MapCache {
        map: std::cell::RefCell<std::collections::HashMap<gix_hash::ObjectId, gix_hash::ObjectId>>,
    }

    impl IndexCache for MapCache {
        fn get_index(&self, tree: gix_hash::ObjectId) -> Option<gix_hash::ObjectId> {
            self.map.borrow().get(&tree).copied()
        }
        fn set_index(&self, tree: gix_hash::ObjectId, index: gix_hash::ObjectId) {
            self.map.borrow_mut().insert(tree, index);
        }
    }

    fn objects(repo: &gix::Repository) -> &impl Objects {
        &repo.objects
    }

    fn commit_tree(repo: &gix::Repository, files: &[(&str, &str)]) -> gix_hash::ObjectId {
        let empty = gix_object::Write::write(
            &repo.objects,
            &gix_object::Tree {
                entries: Vec::new(),
            },
        )
        .unwrap();
        let mut builder = repo.edit_tree(empty).unwrap();
        for (path, content) in files {
            let oid = josh_gix_ext::write_blob(&repo.objects, content.as_bytes()).unwrap();
            builder
                .upsert(*path, gix::objs::tree::EntryKind::Blob, oid)
                .unwrap();
        }
        builder.write().unwrap().detach()
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

        let index = trigram_index(objects(&repo), &cache, &mut Indexer::default(), tree).unwrap();

        // The index format is pinned: cached indexes of older josh versions stay valid only as
        // long as this oid does not change.
        assert_eq!(
            index.to_string(),
            "374073a7525dffb346f59e188a01b5f0ab0459d6"
        );

        let mut sc = SearchCache::default();

        // "Tes" folds to "tes", which lives at its hashed spine path. sub1 is a small
        // directory, so the mirror records it as one coarse blob leaf under its bucket name.
        let leaf = path_entry(
            objects(&repo),
            index,
            std::path::Path::new(&format!("{}/{}", spine_dir(*b"tes"), bucket_name(b"sub1"))),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            gix_object::FindHeader::try_header(&repo.objects, &leaf)
                .unwrap()
                .unwrap()
                .kind,
            gix_object::Kind::Blob
        );

        // Coarse hits expand to every file under the directory; verification is exact.
        let candidates =
            search_candidates(objects(&repo), &mut sc, index, tree, "document").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(objects(&repo), &mut sc, "document", &candidates).unwrap();
        assert_eq!(matches.len(), 2);

        // Trigrams are case-folded, so candidates are a case-insensitive superset ("Test" in
        // file1 makes sub1 a candidate for "test") while regex verification remains exact.
        let candidates = search_candidates(objects(&repo), &mut sc, index, tree, "test").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(objects(&repo), &mut sc, "test", &candidates).unwrap();
        assert!(matches.is_empty());

        let candidates =
            search_candidates(objects(&repo), &mut sc, index, tree, "missingword").unwrap();
        assert!(candidates.is_empty());

        // Short query: every file is a candidate.
        let candidates = search_candidates(objects(&repo), &mut sc, index, tree, "e").unwrap();
        assert_eq!(candidates.len(), 3);

        // Indexing is deterministic and memoization-independent.
        let cold = MapCache::default();
        let index2 = trigram_index(objects(&repo), &cold, &mut Indexer::default(), tree).unwrap();
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

        let index = trigram_index(objects(&repo), &cache, &mut Indexer::default(), tree).unwrap();

        // The bucket is a superset: both members are candidates for a needle in one of them;
        // verification is exact.
        let mut sc = SearchCache::default();
        let candidates =
            search_candidates(objects(&repo), &mut sc, index, tree, "needleword").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"big/target_file"));
        assert!(paths.contains(&format!("big/{}", collider).as_str()));
        let matches = search_matches(objects(&repo), &mut sc, "needleword", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "big/target_file");
    }

    #[test]
    fn colliding_trigrams_share_a_spine_leaf() {
        // Two distinct word trigrams whose spine paths collide: guaranteed by pigeonhole for
        // any geometry up to 15 bits (37^3 word trigrams > 2^15 buckets).
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
        let mut seen: HashMap<[u8; SPINE_LEVELS], [u8; 3]> = HashMap::new();
        let (t1, t2) = 'found: {
            for a in ALPHABET {
                for b in ALPHABET {
                    for c in ALPHABET {
                        let t = [*a, *b, *c];
                        if let Some(prev) = seen.insert(spine_path(t), t) {
                            break 'found (prev, t);
                        }
                    }
                }
            }
            panic!("no spine collision among word trigrams");
        };
        assert_ne!(t1, t2);
        assert_eq!(spine_path(t1), spine_path(t2));

        // A file containing only the colliding trigram is a candidate for the other one;
        // verification is exact.
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();
        let q1 = std::str::from_utf8(&t1).unwrap().to_owned();
        let q2 = std::str::from_utf8(&t2).unwrap().to_owned();
        let tree = commit_tree(
            &repo,
            &[
                ("one", &format!("xx {} yy", q1)),
                ("two", &format!("xx {} yy", q2)),
            ],
        );
        let index = trigram_index(objects(&repo), &cache, &mut Indexer::default(), tree).unwrap();

        let mut sc = SearchCache::default();
        let candidates = search_candidates(objects(&repo), &mut sc, index, tree, &q1).unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["one", "two"]);
        let matches = search_matches(objects(&repo), &mut sc, &q1, &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "one");
    }

    #[test]
    fn curated_punctuation_is_selective() {
        // The curated fold keeps brackets and operators distinct, so a punctuation-shaped
        // query does not degrade to its bare word: a file containing `needle` without the
        // surrounding punctuation is not a candidate for `->needle(`.
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();

        // Files at the root are always indexed per-file; coarse granularity only applies to
        // subdirectories.
        let tree = commit_tree(
            &repo,
            &[
                ("hit.c", "ptr->needle(x);"),
                ("miss.c", "a needle in plain text"),
            ],
        );
        let index = trigram_index(objects(&repo), &cache, &mut Indexer::default(), tree).unwrap();

        let mut sc = SearchCache::default();
        let candidates =
            search_candidates(objects(&repo), &mut sc, index, tree, r"->needle\(").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["hit.c"]);
        let matches = search_matches(objects(&repo), &mut sc, r"->needle\(", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "hit.c");

        // The bare word still finds both.
        let candidates = search_candidates(objects(&repo), &mut sc, index, tree, "needle").unwrap();
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn regex_queries_filter_candidates_and_verify_matches() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();
        let tree = commit_tree(
            &repo,
            &[
                ("concat", "foo crosses the gap to bar"),
                ("foo", "foo alone"),
                ("bar", "bar alone"),
                ("class-c", "abce"),
                ("class-d", "abde"),
                ("alpha", "alpha"),
                ("omega", "omega"),
                ("case", "skip\nNeEdLe42\nNEEDLE7"),
                ("unrelated", "nothing relevant"),
            ],
        );
        let index = trigram_index(objects(&repo), &cache, &mut Indexer::default(), tree).unwrap();
        let mut search_cache = SearchCache::default();

        let candidates =
            search_candidates(objects(&repo), &mut search_cache, index, tree, "foo.*bar").unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec!["concat"]
        );

        let candidates =
            search_candidates(objects(&repo), &mut search_cache, index, tree, "ab[cd]e").unwrap();
        let matches =
            search_matches(objects(&repo), &mut search_cache, "ab[cd]e", &candidates).unwrap();
        assert_eq!(
            matches
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec!["class-c", "class-d"]
        );

        let candidates = search_candidates(
            objects(&repo),
            &mut search_cache,
            index,
            tree,
            "alpha|omega",
        )
        .unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "omega"]
        );

        let candidates = search_candidates(
            objects(&repo),
            &mut search_cache,
            index,
            tree,
            r"(?i)^needle[0-9]{2}$",
        )
        .unwrap();
        let matches = search_matches(
            objects(&repo),
            &mut search_cache,
            r"(?i)^needle[0-9]{2}$",
            &candidates,
        )
        .unwrap();
        assert_eq!(
            matches,
            vec![("case".to_owned(), vec![(2, "NeEdLe42".to_owned())])]
        );

        // A short alternation branch has no trigram, so the conservative query admits all files.
        let candidates =
            search_candidates(objects(&repo), &mut search_cache, index, tree, "foo|x").unwrap();
        assert_eq!(candidates.len(), 9);

        assert!(search_candidates(objects(&repo), &mut search_cache, index, tree, "(").is_err());
        assert!(ChangeSweep::new("(").is_err());
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

        let index = trigram_index(objects(&repo), &cache, &mut Indexer::default(), tree).unwrap();

        // Fine: "d07" (from uniqueword07) mirrors big's structure down to the file's bucket.
        let leaf = path_entry(
            objects(&repo),
            index,
            std::path::Path::new(&format!(
                "{}/{}/{}",
                spine_dir(*b"d07"),
                bucket_name(b"big"),
                bucket_name(b"file_07")
            )),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            gix_object::FindHeader::try_header(&repo.objects, &leaf)
                .unwrap()
                .unwrap()
                .kind,
            gix_object::Kind::Blob
        );

        // Coarse: "nee" (from needleinsmall) records small as a blob leaf, not a subtree.
        let leaf = path_entry(
            objects(&repo),
            index,
            std::path::Path::new(&format!("{}/{}", spine_dir(*b"nee"), bucket_name(b"small"))),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            gix_object::FindHeader::try_header(&repo.objects, &leaf)
                .unwrap()
                .unwrap()
                .kind,
            gix_object::Kind::Blob
        );

        let mut sc = SearchCache::default();

        // Fine candidates stay per-file (modulo bucket collisions) and matches exact.
        let candidates =
            search_candidates(objects(&repo), &mut sc, index, tree, "uniqueword07").unwrap();
        assert!(candidates.iter().any(|(p, _)| p == "big/file_07"));
        let matches = search_matches(objects(&repo), &mut sc, "uniqueword07", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "big/file_07");

        // A coarse hit makes every file under the directory a candidate; verification is
        // exact.
        let candidates =
            search_candidates(objects(&repo), &mut sc, index, tree, "needleinsmall").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["small/a", "small/b"]);
        let matches =
            search_matches(objects(&repo), &mut sc, "needleinsmall", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "small/a");

        // Determinism across memoization states, coarse dirs included.
        let cold = MapCache::default();
        let index2 = trigram_index(objects(&repo), &cold, &mut Indexer::default(), tree).unwrap();
        assert_eq!(index, index2);
    }

    #[test]
    fn change_sweep_matches_brute_force() {
        let (_tmp, repo) = test_repo();
        let cache = MapCache::default();
        let mut indexer = Indexer::default();

        // A history exercising every sweep path: count change in place, rename, removal,
        // and a fine-grained (>16 files) directory changing alongside.
        let filler: Vec<(String, String)> = (0..18)
            .map(|i| (format!("big/f_{:02}", i), format!("filler {}", i)))
            .collect();
        let mut trees = vec![];
        for step in 0..4 {
            let mut files: Vec<(&str, &str)> =
                filler.iter().map(|(p, c)| (&p[..], &c[..])).collect();
            match step {
                0 => files.push(("dir/a", "needle one")),
                1 => files.push(("dir/a", "needle one\nneedle two")),
                2 => files.push(("dir/c", "needle one\nneedle two")),
                _ => files.push(("dir/c", "nothing here")),
            }
            if step >= 1 {
                files.push(("big/f_00x", "needle in big"));
            }
            trees.push(commit_tree(&repo, &files));
        }

        for pattern in ["needle", r"needle (?:one|two)|nothing"] {
            // Brute force: full per-tree match counts with fresh state.
            let counts = |tree: &gix_hash::ObjectId| -> std::collections::BTreeMap<String, usize> {
                let mut sc = SearchCache::default();
                let index = trigram_index(
                    objects(&repo),
                    &MapCache::default(),
                    &mut Indexer::default(),
                    *tree,
                )
                .unwrap();
                let cands =
                    search_candidates(objects(&repo), &mut sc, index, *tree, pattern).unwrap();
                search_matches(objects(&repo), &mut sc, pattern, &cands)
                    .unwrap()
                    .into_iter()
                    .map(|(path, matches)| (path, matches.len()))
                    .collect()
            };

            let mut sweep = ChangeSweep::new(pattern).unwrap();
            let mut sc = SearchCache::default();
            let mut prev: Option<&gix_hash::ObjectId> = None;
            for tree in &trees {
                let index = trigram_index(objects(&repo), &cache, &mut indexer, *tree).unwrap();
                let parents: Vec<gix_hash::ObjectId> = prev.iter().map(|tree| **tree).collect();
                let mut events: Vec<(String, usize, usize)> = sweep
                    .process(objects(&repo), &mut sc, *tree, &parents, *tree, index)
                    .unwrap()
                    .into_iter()
                    .map(|event| (event.path, event.before, event.after))
                    .collect();
                events.sort();

                let before = prev.map(&counts).unwrap_or_default();
                let after = counts(tree);
                let mut expected: Vec<(String, usize, usize)> = vec![];
                for path in before.keys().chain(after.keys()) {
                    let before_count = before.get(path).copied().unwrap_or(0);
                    let after_count = after.get(path).copied().unwrap_or(0);
                    if before_count != after_count
                        && !expected.iter().any(|(existing, _, _)| existing == path)
                    {
                        expected.push((path.clone(), before_count, after_count));
                    }
                }
                expected.sort();
                assert_eq!(events, expected, "pattern {pattern:?}, tree {tree}");
                prev = Some(tree);
            }
        }
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
        let index_a = trigram_index(objects(&repo), &cache, &mut indexer, tree_a).unwrap();

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
        let index_b = trigram_index(objects(&repo), &cache, &mut indexer, tree_b).unwrap();

        // The roots are memoized in the persistent cache. (The sub dirs are below the coarse
        // threshold and live in the Indexer's coarse memo instead — only fine-grained indexes
        // go through the IndexCache.)
        assert!(cache.get_index(tree_b).is_some());

        // Warm (incremental) and cold-built indexes agree bit for bit.
        let cold = MapCache::default();
        let index_b_cold =
            trigram_index(objects(&repo), &cold, &mut Indexer::default(), tree_b).unwrap();
        assert_eq!(index_b, index_b_cold);

        // The incremental index searches correctly — through a cache warmed on commit A,
        // like one GraphQL history+search query warms it: memo entries are content-keyed, so
        // cross-commit reuse must not leak commit A's results into commit B's.
        let mut sc = SearchCache::default();
        let hits = search_candidates(objects(&repo), &mut sc, index_a, tree_a, "delta").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(objects(&repo), &mut sc, index_a, tree_a, "zebra").unwrap();
        let matches = search_matches(objects(&repo), &mut sc, "zebra", &hits).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "sub1/gone");

        let hits = search_candidates(objects(&repo), &mut sc, index_b, tree_b, "delta").unwrap();
        assert_eq!(
            hits.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub2/mod"]
        );
        let hits = search_candidates(objects(&repo), &mut sc, index_b, tree_b, "zebra").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(objects(&repo), &mut sc, index_b, tree_b, "addition").unwrap();
        assert_eq!(
            hits.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub3/new"]
        );

        // Warm-cache results equal fresh-cache results.
        let fresh = search_candidates(
            objects(&repo),
            &mut SearchCache::default(),
            index_b,
            tree_b,
            "delta",
        )
        .unwrap();
        assert_eq!(
            fresh.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub2/mod"]
        );
    }
}
