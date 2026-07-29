//! Trigram based code search index for git repositories.
//!
//! The index of a tree is itself a git tree: an exact inverted index mapping every trigram
//! (3-character window of file content, normalized by [`fold_char`]) to the set of files
//! containing it. Each trigram hashes to a fixed "spine" path of one tree level per
//! [`SPINE_BITS`] entry (see [`spine_path`]), so the index contains
//!
//! ```text
//! <hex(s1)>/<hex(s2)>/<bucket>/<bucket>...
//! ```
//!
//! with the empty blob as the leaf marker. Hashing bounds the spine at a fixed bucket count
//! with uniform fan-out regardless of content; trigrams whose hashes collide share a spine
//! leaf, whose mirror then means "some file below contains some colliding trigram" — search
//! stays a superset and verification exact. The subtree below a trigram's spine levels
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
//! Indexes are built compositionally: every file entry gets a small wrapped trigram tree (its
//! spine with the one-entry mirror `{bucket: empty blob}` at each leaf), a wrap step lifts a
//! child directory's index into its parent's namespace by nesting each spine leaf under the
//! child's bucket, and an n-ary overlay merges the wrapped children into the directory's
//! index in one pass. Git's content addressing shares identical
//! file sets across trigrams and across commits, and the per-(sub)tree memoization through
//! [`IndexCache`] makes indexing incremental with no special path: when a new commit is
//! indexed, unchanged subtrees hit the cache and only the path from a changed blob to the root
//! is recombined. The [`Indexer`] state a caller keeps across [`trigram_index`] calls extends
//! that to the wrap/overlay/blob level, so indexing a chain of commits re-merges only what each
//! commit touched. Trees are hashed and serialized directly with [`gix_object`] and flushed to
//! the object database per call; intermediate results (per-blob trigram trees, partial merges)
//! never touch the ODB.
//!
//! Searching extracts the query's trigrams, resolves each with a single spine lookup, and
//! intersects the mirror subtrees; the resulting candidate files contain every query trigram
//! (modulo hash collisions), leaving only string-level verification to [`search_matches`].
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

/// ASCII punctuation kept distinct by [`fold_char`] instead of collapsing into the class
/// glyph: brackets, operators and common punctuation carry real structure in code, and
/// keeping them distinct is what makes queries like `->foo(` or `a[i]` selective. Changing
/// this set changes the index format (bump josh's cache version).
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
/// text. Used identically on the index side and the query side, which is what keeps the
/// index exact: folding only merges trigram classes, so a query trigram is found in every
/// file containing the query string, candidates are a superset of the true matches, and
/// [`search_matches`] still verifies the original string byte for byte.
fn distinct_trigrams(content: &str) -> BTreeSet<[u8; 3]> {
    let folded: Vec<u8> = content.chars().map(fold_char).collect();
    folded.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
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

fn hex_name(b: u8) -> BString {
    format!("{:02x}", b).into()
}

/// Spine geometry: every trigram maps to a path of `SPINE_BITS.len()` tree levels, level
/// `i`'s name being a `SPINE_BITS[i]`-bit slice of [`trigram_hash`], giving a spine of
/// `2^(sum)` buckets with content-independent fan-out. The widths may differ per level to
/// trade node size against node count. The constant is format-defining: changing it changes
/// every index (bump josh's cache version).
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
/// consecutively from [`trigram_hash`]. Trigrams whose paths collide share a spine leaf; the
/// leaf's mirror is the union of their file sets, so candidates stay a superset and
/// verification exact.
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
/// cutting entry names to two bytes and capping mirror fan-out at 256. Entries whose names
/// collide share a bucket: the bucket means "some member contains the trigram", search
/// expands to all members, and verification stays exact.
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
fn bucketed_entries<'tree>(tree: &'tree git2::Tree<'_>) -> Vec<(String, git2::TreeEntry<'tree>)> {
    tree.iter()
        .filter(|e| {
            matches!(
                e.kind(),
                Some(git2::ObjectType::Blob) | Some(git2::ObjectType::Tree)
            )
        })
        .map(|e| (bucket_name(e.name_bytes()), e))
        .collect()
}

fn tree_entry(filename: BString, oid: gix_hash::ObjectId) -> gix_object::tree::Entry {
    gix_object::tree::Entry {
        mode: gix_object::tree::EntryKind::Tree.into(),
        filename,
        oid,
    }
}

/// Indexing state, meant to be kept alive for many [`trigram_index`] calls (josh keeps one per
/// transaction): unchanged inputs then hit the memos below instead of being rebuilt, which is
/// what makes indexing a chain of commits incremental beyond the per-tree [`IndexCache`].
///
/// All state is only ever ADDED to, so the sole invariant is that entries stay readable:
/// memoized oids must resolve through `pending`, `trees` or the object database of the
/// repository the state is used with. [`flush`](Run::flush) preserves this when it evicts
/// written objects from `pending`.
#[derive(Default)]
pub struct Indexer {
    /// Objects not yet written to the ODB: content hash -> (kind, serialized bytes). Entries
    /// are evicted once flushed (they are readable from the ODB then); what remains are
    /// intermediate results that no persisted index references.
    pending: HashMap<gix_hash::ObjectId, (gix_object::Kind, Vec<u8>)>,
    /// Parsed-tree cache over `pending` + ODB, so the merge machinery does not re-parse the
    /// same spine nodes over and over. Shares one allocation per tree via [`Arc`](std::sync::Arc).
    trees: HashMap<gix_hash::ObjectId, std::sync::Arc<gix_object::Tree>>,
    /// Source tree -> index, for trees indexed or cache-resolved with this state.
    tree_memo: HashMap<git2::Oid, gix_hash::ObjectId>,
    /// Source blob and entry name -> the file's wrapped trigram tree. Keyed on the name too
    /// because the mirrors inside already carry it (the wrap step is fused into the blob spine
    /// construction).
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

/// Directories at or below BOTH limits are recorded at directory granularity: their mirrors
/// hold one leaf for the whole directory instead of one per file. Search then verifies every
/// file under a matched coarse directory, so the limits bound that extra verification work.
const COARSE_MAX_FILES: u64 = 16;
const COARSE_MAX_BYTES: u64 = 64 * 1024;

/// One [`trigram_index`] call: the persistent [`Indexer`] state plus the per-call repository,
/// cache and root bookkeeping.
struct Run<'a> {
    repo: &'a git2::Repository,
    cache: &'a dyn IndexCache,
    ix: &'a mut Indexer,
    /// `(source tree, index)` pairs in build order, memoized persistently by
    /// [`flush`](Run::flush) only after their objects have been written: an [`IndexCache`]
    /// entry must never point at objects missing from the ODB.
    roots: Vec<(git2::Oid, gix_hash::ObjectId)>,
}

impl<'a> Run<'a> {
    /// Serialize `entries` as a tree into `pending` and return its hash. Entries are sorted into
    /// git's canonical order (a directory compares as its name plus `/`) — index trees mix blob
    /// and tree entries, where plain name order differs from canonical order.
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
            let odb = self.repo.odb()?;
            let obj = odb.read(to_git2(oid))?;
            gix_object::TreeRef::from_bytes(obj.data(), gix_hash::Kind::Sha1)?.into_owned()
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

    /// The wrapped trigram tree of one file entry: for each trigram of `content`, its
    /// [`spine_path`] with the one-entry mirror `{bucket: empty blob}` as
    /// the leaf. The wrap step is fused in — every leaf is the same mirror tree, written once
    /// — so the intermediate per-blob tree that [`wrap`](Run::wrap) would immediately rewrite
    /// is never built.
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
    fn subtree_stats(&mut self, tree: &git2::Tree) -> anyhow::Result<(u64, u64)> {
        if let Some(stats) = self.ix.stats_memo.get(&tree.id()) {
            return Ok(*stats);
        }
        let odb = self.repo.odb()?;
        let (mut files, mut bytes) = (0u64, 0u64);
        for entry in tree.iter() {
            match entry.kind() {
                Some(git2::ObjectType::Blob) => {
                    files += 1;
                    bytes += odb.read_header(entry.id())?.0 as u64;
                }
                Some(git2::ObjectType::Tree) => {
                    let child = self.repo.find_tree(entry.id())?;
                    let (f, b) = self.subtree_stats(&child)?;
                    files += f;
                    bytes += b;
                }
                _ => {}
            }
        }
        self.ix.stats_memo.insert(tree.id(), (files, bytes));
        Ok((files, bytes))
    }

    /// Whether a child directory is recorded at directory granularity by its parent.
    fn is_small(&mut self, tree: &git2::Tree) -> anyhow::Result<bool> {
        let (files, bytes) = self.subtree_stats(tree)?;
        Ok(files <= COARSE_MAX_FILES && bytes <= COARSE_MAX_BYTES)
    }

    /// The coarse index of a tree: the whole subtree treated as one pseudo-file — the spine of
    /// every trigram occurring anywhere below it, with empty-blob leaves and no mirror
    /// structure. Wrapping it under the directory's name yields `{name: empty blob}` mirrors:
    /// a blob leaf where the source has a directory, meaning "some file under here contains
    /// the trigram".
    fn coarse_index(&mut self, tree: &git2::Tree) -> anyhow::Result<gix_hash::ObjectId> {
        if let Some(id) = self.ix.coarse_memo.get(&tree.id()) {
            return Ok(*id);
        }

        let mut spines = Vec::with_capacity(tree.len());
        for entry in tree.iter() {
            let name = entry.name()?.to_owned();
            match entry.kind() {
                Some(git2::ObjectType::Blob) => {
                    let content = get_blob(self.repo, tree, &name);
                    spines.push(self.blob_spine(entry.id(), &content));
                }
                Some(git2::ObjectType::Tree) => {
                    let child = self.repo.find_tree(entry.id())?;
                    spines.push(self.coarse_index(&child)?);
                }
                _ => {}
            }
        }
        let index = self.overlay_many(spines)?;

        self.ix.coarse_memo.insert(tree.id(), index);
        Ok(index)
    }

    /// Lift a child directory's index into its parent's namespace: rewrite the spine
    /// levels, nesting each mirror `M` as the single-entry tree `{bucket: M}`. File entries
    /// never come through here — [`index_blob`](Run::index_blob) builds their wrapped form
    /// directly.
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

    /// Merge index trees, all inputs at once: entries are grouped by name across every input
    /// and each group merges recursively, so every result node is written exactly once (a
    /// pairwise fold would rewrite the accumulated spine once per input). Same-name entries
    /// are either trees (spine levels merge recursively; below the spine the wrapped mirrors
    /// of a directory's children have disjoint names) or identical coarse leaves (the empty
    /// blob at the same spine position of several coarse spines), which collapse.
    fn overlay_many(
        &mut self,
        mut inputs: Vec<gix_hash::ObjectId>,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        // The merge is a union: order does not matter, duplicates and empties contribute
        // nothing. Normalizing the key maximizes memo hits.
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
    fn index_tree(&mut self, tree: &git2::Tree) -> anyhow::Result<gix_hash::ObjectId> {
        if let Some(id) = self.ix.tree_memo.get(&tree.id()) {
            return Ok(*id);
        }
        if let Some(cached) = self.cache.get_index(tree.id()) {
            let id = to_gix(cached);
            self.ix.tree_memo.insert(tree.id(), id);
            return Ok(id);
        }

        // Mirror entries are named by bucket (name hash); bucketed_entries is the mapping and
        // the search side derives the same one from the source tree. First decide what each
        // entry contributes: files and small directories contribute blob leaves, large
        // directories mirror subtrees — except in a bucket that mixes both kinds, where the
        // large directories degrade to coarse leaves so the bucket's entries stay mergeable.
        let entries = bucketed_entries(tree);
        let mut leaf_buckets = HashSet::new();
        for (bucket, entry) in &entries {
            let is_leaf = match entry.kind() {
                Some(git2::ObjectType::Blob) => true,
                Some(git2::ObjectType::Tree) => {
                    let child_tree = self.repo.find_tree(entry.id())?;
                    self.is_small(&child_tree)?
                }
                _ => unreachable!(),
            };
            if is_leaf {
                leaf_buckets.insert(bucket.clone());
            }
        }

        let mut wrapped = Vec::with_capacity(entries.len());
        for (bucket, entry) in &entries {
            match entry.kind() {
                Some(git2::ObjectType::Blob) => {
                    let name = entry.name()?;
                    let content = get_blob(self.repo, tree, name);
                    wrapped.push(self.index_blob(entry.id(), bucket, &content));
                }
                Some(git2::ObjectType::Tree) => {
                    let child_tree = self.repo.find_tree(entry.id())?;
                    // Small directories are recorded at directory granularity: one coarse
                    // leaf for the whole directory instead of per-file mirrors. Large ones
                    // too when their bucket also holds leaf contributions.
                    let coarse = self.is_small(&child_tree)? || leaf_buckets.contains(bucket);
                    let child = if coarse {
                        self.coarse_index(&child_tree)?
                    } else {
                        self.index_tree(&child_tree)?
                    };
                    wrapped.push(self.wrap(bucket, child)?);
                }
                _ => unreachable!(),
            };
        }
        let index = self.overlay_many(wrapped)?;

        self.ix.tree_memo.insert(tree.id(), index);
        self.roots.push((tree.id(), index));
        Ok(index)
    }

    /// Write every pending object reachable from a built index to the object database, then
    /// memoize the `(source tree, index)` pairs. Written objects are evicted from `pending` (the
    /// ODB serves them from now on); what stays pending is only reachable from intermediate
    /// results and is never written.
    fn flush(&mut self) -> anyhow::Result<()> {
        if self.roots.is_empty() {
            return Ok(());
        }
        let odb = self.repo.odb()?;

        // Index leaves are the empty blob, and an empty directory's index is the empty tree;
        // neither lives in `pending`, so make sure both exist before anything references them.
        for (kind, id) in [
            (git2::ObjectType::Blob, empty_blob()),
            (git2::ObjectType::Tree, empty_tree()),
        ] {
            if !odb.exists(to_git2(id)) {
                odb.write(kind, &[])?;
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
            let git2_kind = match kind {
                gix_object::Kind::Tree => git2::ObjectType::Tree,
                _ => git2::ObjectType::Blob,
            };
            if !odb.exists(to_git2(id)) {
                odb.write(git2_kind, data)?;
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

pub fn trigram_index<'a>(
    repo: &'a git2::Repository,
    cache: &dyn IndexCache,
    indexer: &mut Indexer,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    let mut run = Run {
        repo,
        cache,
        ix: indexer,
        roots: Vec::new(),
    };
    let index = run.index_tree(&tree)?;
    run.flush()?;
    Ok(repo.find_tree(to_git2(index))?)
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

/// Exact candidate files for `searchstring`: files containing every trigram of the query.
///
/// Queries shorter than three bytes (or without any valid-UTF-8 window) have no trigrams; every
/// file of `source_tree` is a candidate then, and [`search_matches`] does the filtering.
pub fn search_candidates(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    index_tree: &git2::Tree,
    source_tree: &git2::Tree,
    searchstring: &str,
) -> anyhow::Result<Vec<(String, git2::Oid)>> {
    let key = (searchstring.to_owned(), index_tree.id(), source_tree.id());
    if let Some(hit) = cache.candidates.get(&key) {
        return Ok((**hit).clone());
    }

    let results = match query_roots(repo, cache, index_tree, searchstring)? {
        // No usable trigrams: every file is a candidate.
        None => (*all_paths(repo, cache, source_tree.id())?).clone(),
        Some(roots) if roots.is_empty() => vec![],
        Some(roots) => (*walk(repo, cache, roots, source_tree)?).clone(),
    };

    cache
        .candidates
        .insert(key, std::sync::Arc::new(results.clone()));
    Ok(results)
}

/// The normalized mirror-root vector `searchstring` intersects in `index_tree`: sorted,
/// deduplicated and capped, exactly the input [`search_candidates`] hands to the intersection.
/// `None` means the query has no usable trigrams (every file is a candidate); an empty vector
/// means some trigram is absent (no file can match). Equal vectors for two indexes imply
/// equal candidate path sets — the basis for change detection across commits.
pub fn query_roots(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    index_tree: &git2::Tree,
    searchstring: &str,
) -> anyhow::Result<Option<Vec<git2::Oid>>> {
    let trigrams = distinct_trigrams(searchstring);
    if trigrams.is_empty() {
        return Ok(None);
    }

    // Resolve each trigram's spine levels through the parsed-mirror cache (spine names
    // are the same fixed-width hex as bucket names): each distinct spine node is parsed once
    // per process instead of once per lookup.
    let mut roots = vec![];
    for t in &trigrams {
        let mut node = index_tree.id();
        for b in spine_path(*t) {
            let entries = mirror_entries(repo, cache, node)?;
            match entries.binary_search_by_key(&hex_pair(b), |e| e.name) {
                Ok(i) => node = entries[i].oid,
                // A trigram absent from the index cannot occur in any file.
                Err(_) => return Ok(Some(vec![])),
            }
        }
        roots.push(node);
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
            .map(|&oid| anyhow::Ok((mirror_entries(repo, cache, oid)?.len(), oid)))
            .collect::<Result<Vec<_>, _>>()?;
        sized.sort();
        roots = sized
            .into_iter()
            .take(MAX_INTERSECT)
            .map(|(_, oid)| oid)
            .collect();
    }

    Ok(Some(roots))
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
    repo: &git2::Repository,
    cache: &mut SearchCache,
    oid: git2::Oid,
) -> anyhow::Result<std::sync::Arc<Vec<MirrorEntry>>> {
    if let Some(hit) = cache.mirrors.get(&oid) {
        return Ok(hit.clone());
    }
    let tree = repo.find_tree(oid)?;
    let mut entries = Vec::with_capacity(tree.len());
    for entry in tree.iter() {
        let name = entry.name_bytes();
        if let [a, b] = name {
            entries.push(MirrorEntry {
                name: [*a, *b],
                oid: entry.id(),
                tree: entry.kind() == Some(git2::ObjectType::Tree),
            });
        }
    }
    let entries = std::sync::Arc::new(entries);
    cache.mirrors.insert(oid, entries.clone());
    Ok(entries)
}

/// The candidate file paths (relative to `source`) present in ALL of the mirror trees
/// `roots`, memoized on the normalized root set and the source tree. Mirror entries are named
/// by bucket; `source` provides the bucket -> entries mapping per level. A blob entry expands
/// to every bucket member (each file directly, each directory — coarse leaf — to all files
/// under it); a tree entry recurses into every large-directory member. Because results are
/// relative and keyed by content, walks are shared across commits and across trigrams
/// wherever the subtrees agree.
///
/// The intersection is a k-way merge over the mirrors' entry lists: bucket names are fixed
/// width, so git's canonical entry order is plain byte order and one linear pass replaces
/// per-bucket lookups — this loop runs once per commit of a history sweep, so its constant
/// factor matters.
fn walk(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    mut roots: Vec<git2::Oid>,
    source: &git2::Tree,
) -> anyhow::Result<std::sync::Arc<Vec<(String, git2::Oid)>>> {
    // The intersection is a set operation: normalize the key. Identical mirrors (trigrams
    // with the same file set) intersect to themselves, so duplicates collapse.
    roots.sort();
    roots.dedup();
    let key = (roots.clone(), source.id());
    if let Some(hit) = cache.walks.get(&key) {
        return Ok(hit.clone());
    }

    let entry_lists = roots
        .iter()
        .map(|oid| mirror_entries(repo, cache, *oid))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Source entries by bucket byte; a flat table instead of a hash map keyed by name.
    let mut by_bucket: Vec<Vec<git2::TreeEntry>> = (0..256).map(|_| Vec::new()).collect();
    for entry in source.iter() {
        if matches!(
            entry.kind(),
            Some(git2::ObjectType::Blob) | Some(git2::ObjectType::Tree)
        ) {
            by_bucket[bucket_byte(entry.name_bytes()) as usize].push(entry);
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
                    if member.kind() != Some(git2::ObjectType::Tree) {
                        continue;
                    }
                    let name = member.name()?;
                    let source_child = repo.find_tree(member.id())?;
                    let sub = walk(repo, cache, child_roots.clone(), &source_child)?;
                    out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
                }
            } else {
                for member in members {
                    let name = member.name()?;
                    match member.kind() {
                        Some(git2::ObjectType::Blob) => out.push((name.to_owned(), member.id())),
                        Some(git2::ObjectType::Tree) => {
                            let sub = all_paths(repo, cache, member.id())?;
                            out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
                        }
                        _ => {}
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

/// All (relative path, blob) pairs under the tree `oid`, memoized per tree: the expansion of
/// coarse leaves and the fallback for queries without trigrams.
fn all_paths(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    oid: git2::Oid,
) -> anyhow::Result<std::sync::Arc<Vec<(String, git2::Oid)>>> {
    if let Some(hit) = cache.all_paths.get(&oid) {
        return Ok(hit.clone());
    }
    let tree = repo.find_tree(oid)?;
    let mut out = vec![];
    for entry in tree.iter() {
        let name = entry.name()?;
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                let sub = all_paths(repo, cache, entry.id())?;
                out.extend(sub.iter().map(|(p, b)| (join_path(name, p), *b)));
            }
            Some(git2::ObjectType::Blob) => out.push((name.to_owned(), entry.id())),
            _ => {}
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
    repo: &git2::Repository,
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
            let b = blob_content(repo, *blob);
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

/// The number of lines of blob `oid` matching `query`, through the per-blob match memo.
fn match_count(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    query: &str,
    blob: git2::Oid,
) -> anyhow::Result<usize> {
    let m = search_matches(repo, cache, query, &[(String::new(), blob)])?;
    Ok(m.first().map(|r| r.1.len()).unwrap_or(0))
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
    repo: &git2::Repository,
    cache: &mut SearchCache,
    a: git2::Oid,
    b: git2::Oid,
    node: &mut BucketTrie,
) -> anyhow::Result<()> {
    if a == b {
        return Ok(());
    }
    let la = mirror_entries(repo, cache, a)?;
    let lb = mirror_entries(repo, cache, b)?;
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
            Some((ea, eb)) => diff_mirrors(repo, cache, ea, eb, child)?,
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
    repo: &git2::Repository,
    cache: &mut SearchCache,
    mut roots: Vec<git2::Oid>,
    source: &git2::Tree,
    trie: &BucketTrie,
    out: &mut Vec<(String, git2::Oid)>,
) -> anyhow::Result<()> {
    roots.sort();
    roots.dedup();
    if trie.changed {
        out.extend(walk(repo, cache, roots, source)?.iter().cloned());
        return Ok(());
    }

    let entry_lists = roots
        .iter()
        .map(|oid| mirror_entries(repo, cache, *oid))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut by_bucket: Vec<Vec<git2::TreeEntry>> = (0..256).map(|_| Vec::new()).collect();
    for entry in source.iter() {
        if matches!(
            entry.kind(),
            Some(git2::ObjectType::Blob) | Some(git2::ObjectType::Tree)
        ) {
            by_bucket[bucket_byte(entry.name_bytes()) as usize].push(entry);
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
                if member.kind() != Some(git2::ObjectType::Tree) {
                    continue;
                }
                let mname = member.name()?;
                let source_child = repo.find_tree(member.id())?;
                let mut sub_out = vec![];
                walk_restricted(
                    repo,
                    cache,
                    child_roots.clone(),
                    &source_child,
                    sub,
                    &mut sub_out,
                )?;
                out.extend(sub_out.into_iter().map(|(p, b)| (join_path(mname, &p), b)));
            }
        } else {
            for member in members {
                let mname = member.name()?;
                match member.kind() {
                    Some(git2::ObjectType::Blob) => out.push((mname.to_owned(), member.id())),
                    Some(git2::ObjectType::Tree) => {
                        let sub_paths = all_paths(repo, cache, member.id())?;
                        out.extend(sub_paths.iter().map(|(p, b)| (join_path(mname, p), *b)));
                    }
                    _ => {}
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
    repo: &git2::Repository,
    c_oid: git2::Oid,
    p_oid: git2::Oid,
    cands: &[(String, git2::Oid)],
    depth: usize,
    out: &mut Vec<(String, git2::Oid)>,
) -> anyhow::Result<bool> {
    if c_oid == p_oid {
        out.extend_from_slice(cands);
        return Ok(true);
    }
    let c_tree = repo.find_tree(c_oid)?;
    let p_tree = repo.find_tree(p_oid)?;

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

        let ids = match (c_tree.get_name(comp), p_tree.get_name(comp)) {
            (Some(ce), Some(pe)) => (ce.id(), pe.id()),
            _ => return Ok(false),
        };
        let is_leaf = cands[i].0.split('/').count() == depth + 1;
        if is_leaf {
            out.push((cands[i].0.clone(), ids.0));
        } else if !refresh_group(repo, ids.0, ids.1, &cands[i..j], depth + 1, out)? {
            return Ok(false);
        }
        i = j;
    }
    Ok(true)
}

/// Per-commit sweep state: the source tree, the query's mirror roots (unsorted, aligned
/// with the query's spine path list; `None` while any path is absent), and the candidate
/// pairs.
struct SweepState {
    tree: git2::Oid,
    roots: Option<Vec<git2::Oid>>,
    cands: std::sync::Arc<Vec<(String, git2::Oid)>>,
}

/// Pickaxe-style change detection over a history: feed commits parents-first through
/// [`process`](ChangeSweep::process) and collect, per non-merge commit, the files whose
/// matching-line count changed against the first parent.
///
/// Content addressing keeps the per-commit cost proportional to what changed: equal
/// per-trigram mirror roots prove the candidate path set unchanged (only blob oids are
/// re-resolved, skipping identical subtrees); differing roots are diffed mirror-by-mirror
/// into a bucket trie, and only trie scope is re-walked while the rest of the parent's
/// candidates are spliced through. Reported events are independent of candidate superset
/// choices: false candidates verify to zero matches on both sides and never emit.
pub struct ChangeSweep {
    query: String,
    /// The query's distinct trigram spine paths (colliding trigrams collapse to one path,
    /// and so would resolve to the same root anyway).
    spine_paths: BTreeSet<[u8; SPINE_LEVELS]>,
    store: HashMap<git2::Oid, SweepState>,
}

impl ChangeSweep {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_owned(),
            spine_paths: distinct_trigrams(query)
                .iter()
                .map(|t| spine_path(*t))
                .collect(),
            store: HashMap::new(),
        }
    }

    /// The query's mirror root per spine path, aligned with `self.spine_paths`; `None` if
    /// any path is absent (no file can match).
    fn trigram_roots(
        &self,
        repo: &git2::Repository,
        cache: &mut SearchCache,
        index_oid: git2::Oid,
    ) -> anyhow::Result<Option<Vec<git2::Oid>>> {
        let mut roots = Vec::with_capacity(self.spine_paths.len());
        for path in &self.spine_paths {
            let mut node = index_oid;
            for b in path {
                let entries = mirror_entries(repo, cache, node)?;
                match entries.binary_search_by_key(&hex_pair(*b), |e| e.name) {
                    Ok(i) => node = entries[i].oid,
                    Err(_) => return Ok(None),
                }
            }
            roots.push(node);
        }
        Ok(Some(roots))
    }

    pub fn process(
        &mut self,
        repo: &git2::Repository,
        cache: &mut SearchCache,
        commit_id: git2::Oid,
        parent_ids: &[git2::Oid],
        source_tree: &git2::Tree,
        index_tree: &git2::Tree,
    ) -> anyhow::Result<Vec<ChangeEvent>> {
        let roots = if self.spine_paths.is_empty() {
            None
        } else {
            self.trigram_roots(repo, cache, index_tree.id())?
        };
        let parent = parent_ids.first().and_then(|p| self.store.get(p));

        let cands = 'cands: {
            if let (Some(rc), Some(sp)) = (&roots, parent) {
                if sp.tree == source_tree.id() {
                    break 'cands sp.cands.clone();
                }
                if let Some(rp) = &sp.roots {
                    if rp == rc {
                        // Identical roots: same candidate paths, only blobs may differ.
                        let mut out = Vec::with_capacity(sp.cands.len());
                        if refresh_group(repo, source_tree.id(), sp.tree, &sp.cands, 0, &mut out)? {
                            break 'cands std::sync::Arc::new(out);
                        }
                    } else {
                        // Diff the changed mirrors into a trie, splice: keep and re-resolve
                        // the parent's candidates outside the trie, re-walk only inside it.
                        let mut trie = BucketTrie::default();
                        for (a, b) in rc.iter().zip(rp.iter()) {
                            diff_mirrors(repo, cache, *a, *b, &mut trie)?;
                        }
                        let kept: Vec<(String, git2::Oid)> = sp
                            .cands
                            .iter()
                            .filter(|(p, _)| !path_changed(&trie, p))
                            .cloned()
                            .collect();
                        let mut out = Vec::with_capacity(kept.len());
                        if refresh_group(repo, source_tree.id(), sp.tree, &kept, 0, &mut out)? {
                            walk_restricted(repo, cache, rc.clone(), source_tree, &trie, &mut out)?;
                            out.sort();
                            out.dedup();
                            break 'cands std::sync::Arc::new(out);
                        }
                    }
                }
            }
            // Fallbacks: absent trigrams mean no candidates; otherwise a full walk.
            if roots.is_none() && !self.spine_paths.is_empty() {
                break 'cands std::sync::Arc::new(vec![]);
            }
            std::sync::Arc::new(search_candidates(
                repo,
                cache,
                index_tree,
                source_tree,
                &self.query,
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
            events = self.diff_events(repo, cache, &pcands, &cands)?;
        }

        self.store.insert(
            commit_id,
            SweepState {
                tree: source_tree.id(),
                roots,
                cands,
            },
        );
        Ok(events)
    }

    fn diff_events(
        &self,
        repo: &git2::Repository,
        cache: &mut SearchCache,
        pcands: &[(String, git2::Oid)],
        cands: &[(String, git2::Oid)],
    ) -> anyhow::Result<Vec<ChangeEvent>> {
        let mut events = vec![];
        let (mut i, mut j) = (0, 0);
        while i < cands.len() || j < pcands.len() {
            let (path, before, after) =
                if j >= pcands.len() || (i < cands.len() && cands[i].0 < pcands[j].0) {
                    let (path, blob) = &cands[i];
                    i += 1;
                    (path, 0, match_count(repo, cache, &self.query, *blob)?)
                } else if i >= cands.len() || pcands[j].0 < cands[i].0 {
                    let (path, blob) = &pcands[j];
                    j += 1;
                    (path, match_count(repo, cache, &self.query, *blob)?, 0)
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
                        match_count(repo, cache, &self.query, pblob)?,
                        match_count(repo, cache, &self.query, *blob)?,
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

/// Like [`get_blob`], but by oid: "" if the object is not a blob, binary or not UTF-8.
fn blob_content(repo: &git2::Repository, oid: git2::Oid) -> String {
    let Ok(blob) = repo.find_blob(oid) else {
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
        // as long as these paths do not move; a change requires a josh cache version bump.
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

        let index = trigram_index(&repo, &cache, &mut Indexer::default(), tree.clone()).unwrap();

        // The index format is pinned: cached indexes of older josh versions stay valid only as
        // long as this oid does not change; a change requires a josh cache version bump.
        assert_eq!(
            index.id().to_string(),
            "374073a7525dffb346f59e188a01b5f0ab0459d6"
        );

        let mut sc = SearchCache::default();

        // "Tes" folds to "tes", which lives at its hashed spine path. sub1 is a small
        // directory, so the mirror records it as one coarse blob leaf under its bucket name.
        let leaf = index
            .get_path(std::path::Path::new(&format!(
                "{}/{}",
                spine_dir(*b"tes"),
                bucket_name(b"sub1")
            )))
            .unwrap();
        assert_eq!(leaf.kind(), Some(git2::ObjectType::Blob));

        // Coarse hits expand to every file under the directory; verification is exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "document").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(&repo, &mut sc, "document", &candidates).unwrap();
        assert_eq!(matches.len(), 2);

        // Trigrams are case-folded, so candidates are a case-insensitive superset ("Test" in
        // file1 makes sub1 a candidate for "test") while match verification stays byte-exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "test").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(&repo, &mut sc, "test", &candidates).unwrap();
        assert!(matches.is_empty());

        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "missingword").unwrap();
        assert!(candidates.is_empty());

        // Short query: every file is a candidate.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "e").unwrap();
        assert_eq!(candidates.len(), 3);

        // Indexing is deterministic and memoization-independent: a cold rebuild of the same
        // tree yields the same oid.
        let cold = MapCache::default();
        let index2 = trigram_index(&repo, &cold, &mut Indexer::default(), tree).unwrap();
        assert_eq!(index.id(), index2.id());
    }

    #[test]
    fn bucket_name_basics() {
        // Deterministic, two lowercase hex chars, and a function of the name alone.
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

        let index = trigram_index(&repo, &cache, &mut Indexer::default(), tree.clone()).unwrap();

        // The bucket is a superset: both members are candidates for a needle in one of them;
        // verification is exact.
        let mut sc = SearchCache::default();
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "needleword").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"big/target_file"));
        assert!(paths.contains(&format!("big/{}", collider).as_str()));
        let matches = search_matches(&repo, &mut sc, "needleword", &candidates).unwrap();
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
        let index = trigram_index(&repo, &cache, &mut Indexer::default(), tree.clone()).unwrap();

        let mut sc = SearchCache::default();
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, &q1).unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["one", "two"]);
        let matches = search_matches(&repo, &mut sc, &q1, &candidates).unwrap();
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
        let index = trigram_index(&repo, &cache, &mut Indexer::default(), tree.clone()).unwrap();

        let mut sc = SearchCache::default();
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "->needle(").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["hit.c"]);
        let matches = search_matches(&repo, &mut sc, "->needle(", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "hit.c");

        // The bare word still finds both.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "needle").unwrap();
        assert_eq!(candidates.len(), 2);
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

        let index = trigram_index(&repo, &cache, &mut Indexer::default(), tree.clone()).unwrap();

        // Fine: "d07" (from uniqueword07) mirrors big's structure down to the file's bucket.
        let leaf = index
            .get_path(std::path::Path::new(&format!(
                "{}/{}/{}",
                spine_dir(*b"d07"),
                bucket_name(b"big"),
                bucket_name(b"file_07")
            )))
            .unwrap();
        assert_eq!(leaf.kind(), Some(git2::ObjectType::Blob));

        // Coarse: "nee" (from needleinsmall) records small as a blob leaf, not a subtree.
        let leaf = index
            .get_path(std::path::Path::new(&format!(
                "{}/{}",
                spine_dir(*b"nee"),
                bucket_name(b"small")
            )))
            .unwrap();
        assert_eq!(leaf.kind(), Some(git2::ObjectType::Blob));

        let mut sc = SearchCache::default();

        // Fine candidates stay per-file (modulo bucket collisions) and matches exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "uniqueword07").unwrap();
        assert!(candidates.iter().any(|(p, _)| p == "big/file_07"));
        let matches = search_matches(&repo, &mut sc, "uniqueword07", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "big/file_07");

        // A coarse hit makes every file under the directory a candidate; verification is
        // exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "needleinsmall").unwrap();
        let paths: Vec<&str> = candidates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["small/a", "small/b"]);
        let matches = search_matches(&repo, &mut sc, "needleinsmall", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "small/a");

        // Determinism across memoization states, coarse dirs included.
        let cold = MapCache::default();
        let index2 = trigram_index(&repo, &cold, &mut Indexer::default(), tree).unwrap();
        assert_eq!(index.id(), index2.id());
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

        // Brute force: full per-tree match counts with fresh state.
        let counts = |tree: &git2::Tree| -> std::collections::BTreeMap<String, usize> {
            let mut sc = SearchCache::default();
            let index = trigram_index(
                &repo,
                &MapCache::default(),
                &mut Indexer::default(),
                tree.clone(),
            )
            .unwrap();
            let cands = search_candidates(&repo, &mut sc, &index, tree, "needle").unwrap();
            search_matches(&repo, &mut sc, "needle", &cands)
                .unwrap()
                .into_iter()
                .map(|(p, m)| (p, m.len()))
                .collect()
        };

        let mut sweep = ChangeSweep::new("needle");
        let mut sc = SearchCache::default();
        let mut prev: Option<&git2::Tree> = None;
        for tree in &trees {
            let index = trigram_index(&repo, &cache, &mut indexer, tree.clone()).unwrap();
            let parents: Vec<git2::Oid> = prev.iter().map(|t| t.id()).collect();
            let mut events: Vec<(String, usize, usize)> = sweep
                .process(&repo, &mut sc, tree.id(), &parents, tree, &index)
                .unwrap()
                .into_iter()
                .map(|e| (e.path, e.before, e.after))
                .collect();
            events.sort();

            let before = prev.map(&counts).unwrap_or_default();
            let after = counts(tree);
            let mut expected: Vec<(String, usize, usize)> = vec![];
            for path in before.keys().chain(after.keys()) {
                let b = before.get(path).copied().unwrap_or(0);
                let a = after.get(path).copied().unwrap_or(0);
                if b != a && !expected.iter().any(|(p, _, _)| p == path) {
                    expected.push((path.clone(), b, a));
                }
            }
            expected.sort();
            assert_eq!(events, expected, "at step tree {}", tree.id());
            prev = Some(tree);
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
        let index_a = trigram_index(&repo, &cache, &mut indexer, tree_a.clone()).unwrap();

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
        let index_b = trigram_index(&repo, &cache, &mut indexer, tree_b.clone()).unwrap();

        // The roots are memoized in the persistent cache. (The sub dirs are below the coarse
        // threshold and live in the Indexer's coarse memo instead — only fine-grained indexes
        // go through the IndexCache.)
        assert!(cache.get_index(tree_b.id()).is_some());

        // Warm (incremental) and cold-built indexes agree bit for bit.
        let cold = MapCache::default();
        let index_b_cold =
            trigram_index(&repo, &cold, &mut Indexer::default(), tree_b.clone()).unwrap();
        assert_eq!(index_b.id(), index_b_cold.id());

        // And the incremental index searches correctly — through a cache warmed on commit A,
        // like one GraphQL history+search query warms it: memo entries are content-keyed, so
        // cross-commit reuse must not leak commit A's results into commit B's.
        let mut sc = SearchCache::default();
        let hits = search_candidates(&repo, &mut sc, &index_a, &tree_a, "delta").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(&repo, &mut sc, &index_a, &tree_a, "zebra").unwrap();
        let matches = search_matches(&repo, &mut sc, "zebra", &hits).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "sub1/gone");

        let hits = search_candidates(&repo, &mut sc, &index_b, &tree_b, "delta").unwrap();
        assert_eq!(
            hits.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub2/mod"]
        );
        let hits = search_candidates(&repo, &mut sc, &index_b, &tree_b, "zebra").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(&repo, &mut sc, &index_b, &tree_b, "addition").unwrap();
        assert_eq!(
            hits.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub3/new"]
        );

        // Warm-cache results equal fresh-cache results.
        let fresh = search_candidates(
            &repo,
            &mut SearchCache::default(),
            &index_b,
            &tree_b,
            "delta",
        )
        .unwrap();
        assert_eq!(
            fresh.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec!["sub2/mod"]
        );
    }
}
