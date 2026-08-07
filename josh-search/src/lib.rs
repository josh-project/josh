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

/// The mirror entry name of a source entry: a one-byte FNV-1a hash of its name, as two hex
/// chars. Depending only on the entry's own name keeps mirrors stable when siblings come and
/// go (a dense scheme like positions would rename neighbours on every insertion), while
/// cutting entry names to two bytes and capping mirror fan-out at 256. Entries whose names
/// collide share a bucket: the bucket means "some member contains the trigram", search
/// expands to all members, and verification stays exact.
fn bucket_name(name: &[u8]) -> String {
    let mut h: u32 = 0x811c9dc5;
    for &b in name {
        h = (h ^ b as u32).wrapping_mul(0x01000193);
    }
    format!("{:02x}", (h ^ (h >> 8) ^ (h >> 16) ^ (h >> 24)) & 0xff)
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

    /// The wrapped trigram tree of one file entry: for each trigram of `content`, the spine
    /// path `hex(b1)/hex(b2)/hex(b3)` with the one-entry mirror `{bucket: empty blob}` as
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

    /// Lift a child directory's index into its parent's namespace: rewrite the three spine
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
    /// (query, index tree, source tree) -> sorted candidate paths.
    candidates: HashMap<(String, git2::Oid, git2::Oid), std::sync::Arc<Vec<String>>>,
    /// (mirror roots (normalized), source tree) -> relative candidate paths of the walk.
    walks: HashMap<(Vec<git2::Oid>, git2::Oid), std::sync::Arc<Vec<String>>>,
    /// source tree -> all file paths under it (relative).
    all_paths: HashMap<git2::Oid, std::sync::Arc<Vec<String>>>,
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
) -> anyhow::Result<Vec<String>> {
    let key = (searchstring.to_owned(), index_tree.id(), source_tree.id());
    if let Some(hit) = cache.candidates.get(&key) {
        return Ok((**hit).clone());
    }

    let trigrams = distinct_trigrams(searchstring);

    let results = if trigrams.is_empty() {
        (*all_paths(repo, cache, source_tree.id())?).clone()
    } else {
        let mut roots = vec![];
        let mut absent = false;
        for t in &trigrams {
            let path = format!("{:02x}/{:02x}/{:02x}", t[0], t[1], t[2]);
            match index_tree.get_path(std::path::Path::new(&path)) {
                Ok(entry) => roots.push(entry.id()),
                // A trigram absent from the index cannot occur in any file.
                Err(_) => {
                    absent = true;
                    break;
                }
            }
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
                    .map(|&oid| anyhow::Ok((repo.find_tree(oid)?.len(), oid)))
                    .collect::<Result<Vec<_>, _>>()?;
                sized.sort();
                roots = sized
                    .into_iter()
                    .take(MAX_INTERSECT)
                    .map(|(_, oid)| oid)
                    .collect();
            }

            (*walk(repo, cache, roots, source_tree)?).clone()
        }
    };

    cache
        .candidates
        .insert(key, std::sync::Arc::new(results.clone()));
    Ok(results)
}

/// The bucket -> source entries mapping of one directory level. Colliding names share a
/// bucket, so a mirror entry can resolve to several source entries.
fn buckets_of<'tree>(
    source: &'tree git2::Tree<'_>,
) -> HashMap<String, Vec<git2::TreeEntry<'tree>>> {
    let mut map: HashMap<String, Vec<git2::TreeEntry>> = HashMap::new();
    for (bucket, entry) in bucketed_entries(source) {
        map.entry(bucket).or_default().push(entry);
    }
    map
}

/// The candidate file paths (relative to `source`) present in ALL of the mirror trees
/// `roots`, memoized on the normalized root set and the source tree. Mirror entries are named
/// by bucket; `source` provides the bucket -> entries mapping per level. A blob entry expands
/// to every bucket member (each file directly, each directory — coarse leaf — to all files
/// under it); a tree entry recurses into every large-directory member. Because results are
/// relative and keyed by content, walks are shared across commits and across trigrams
/// wherever the subtrees agree.
fn walk(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    mut roots: Vec<git2::Oid>,
    source: &git2::Tree,
) -> anyhow::Result<std::sync::Arc<Vec<String>>> {
    // The intersection is a set operation: normalize the key. Identical mirrors (trigrams
    // with the same file set) intersect to themselves, so duplicates collapse.
    roots.sort();
    roots.dedup();
    let key = (roots.clone(), source.id());
    if let Some(hit) = cache.walks.get(&key) {
        return Ok(hit.clone());
    }

    let trees = roots
        .iter()
        .map(|oid| repo.find_tree(*oid))
        .collect::<Result<Vec<_>, _>>()?;
    let smallest = trees
        .iter()
        .min_by_key(|t| t.len())
        .expect("roots is never empty");
    let buckets = buckets_of(source);

    let mut out = vec![];
    'entry: for entry in smallest.iter() {
        let bucket = entry.name()?;

        let mut child_roots = Vec::with_capacity(trees.len());
        if trees.len() == 1 {
            child_roots.push(entry.id());
        } else {
            for tree in &trees {
                match tree.get_name(bucket) {
                    Some(other) if other.kind() == entry.kind() => child_roots.push(other.id()),
                    _ => continue 'entry,
                }
            }
        }

        let Some(members) = buckets.get(bucket) else {
            continue;
        };
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                for member in members {
                    if member.kind() != Some(git2::ObjectType::Tree) {
                        continue;
                    }
                    let name = member.name()?;
                    let source_child = repo.find_tree(member.id())?;
                    let sub = walk(repo, cache, child_roots.clone(), &source_child)?;
                    out.extend(sub.iter().map(|p| join_path(name, p)));
                }
            }
            Some(git2::ObjectType::Blob) => {
                for member in members {
                    let name = member.name()?;
                    match member.kind() {
                        Some(git2::ObjectType::Blob) => out.push(name.to_owned()),
                        Some(git2::ObjectType::Tree) => {
                            let sub = all_paths(repo, cache, member.id())?;
                            out.extend(sub.iter().map(|p| join_path(name, p)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // Mirror iteration follows bucket (hash) order; keep results in path order.
    out.sort();
    out.dedup();
    let out = std::sync::Arc::new(out);
    cache.walks.insert(key, out.clone());
    Ok(out)
}

/// All file paths under the tree `oid` (relative), memoized per tree: the expansion of coarse
/// leaves and the fallback for queries without trigrams.
fn all_paths(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    oid: git2::Oid,
) -> anyhow::Result<std::sync::Arc<Vec<String>>> {
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
                out.extend(sub.iter().map(|p| join_path(name, p)));
            }
            Some(git2::ObjectType::Blob) => out.push(name.to_owned()),
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
/// under, so verifying a history costs one scan per distinct blob.
pub fn search_matches(
    repo: &git2::Repository,
    cache: &mut SearchCache,
    tree: &git2::Tree,
    searchstring: &str,
    candidates: &[String],
) -> anyhow::Result<SearchMatchesResult> {
    let mut results = vec![];

    for c in candidates {
        let Ok(entry) = tree.get_path(std::path::Path::new(&c)) else {
            continue;
        };
        let key = (searchstring.to_owned(), entry.id());
        let bresults = if let Some(hit) = cache.blob_matches.get(&key) {
            hit.clone()
        } else {
            let b = blob_content(repo, entry.id());
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
            "03111ef5ca624b955d799a80181c1bf0c2099fcd"
        );

        let mut sc = SearchCache::default();

        // "Tes" folds to "tes", which lives at 74/65/73 in the hex spine. sub1 is a small
        // directory, so the mirror records it as one coarse blob leaf under its bucket name.
        let leaf = index
            .get_path(std::path::Path::new(&format!(
                "74/65/73/{}",
                bucket_name(b"sub1")
            )))
            .unwrap();
        assert_eq!(leaf.kind(), Some(git2::ObjectType::Blob));

        // Coarse hits expand to every file under the directory; verification is exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "document").unwrap();
        assert_eq!(candidates, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(&repo, &mut sc, &tree, "document", &candidates).unwrap();
        assert_eq!(matches.len(), 2);

        // Trigrams are case-folded, so candidates are a case-insensitive superset ("Test" in
        // file1 makes sub1 a candidate for "test") while match verification stays byte-exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "test").unwrap();
        assert_eq!(candidates, vec!["sub1/file1", "sub1/file2"]);
        let matches = search_matches(&repo, &mut sc, &tree, "test", &candidates).unwrap();
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
        assert!(candidates.contains(&"big/target_file".to_owned()));
        assert!(candidates.contains(&format!("big/{}", collider)));
        let matches = search_matches(&repo, &mut sc, &tree, "needleword", &candidates).unwrap();
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

        let index = trigram_index(&repo, &cache, &mut Indexer::default(), tree.clone()).unwrap();

        // Fine: "d07" (from uniqueword07) mirrors big's structure down to the file's bucket.
        let leaf = index
            .get_path(std::path::Path::new(&format!(
                "64/30/37/{}/{}",
                bucket_name(b"big"),
                bucket_name(b"file_07")
            )))
            .unwrap();
        assert_eq!(leaf.kind(), Some(git2::ObjectType::Blob));

        // Coarse: "nee" (from needleinsmall) records small as a blob leaf, not a subtree.
        let leaf = index
            .get_path(std::path::Path::new(&format!(
                "6e/65/65/{}",
                bucket_name(b"small")
            )))
            .unwrap();
        assert_eq!(leaf.kind(), Some(git2::ObjectType::Blob));

        let mut sc = SearchCache::default();

        // Fine candidates stay per-file (modulo bucket collisions) and matches exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "uniqueword07").unwrap();
        assert!(candidates.contains(&"big/file_07".to_owned()));
        let matches = search_matches(&repo, &mut sc, &tree, "uniqueword07", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "big/file_07");

        // A coarse hit makes every file under the directory a candidate; verification is
        // exact.
        let candidates = search_candidates(&repo, &mut sc, &index, &tree, "needleinsmall").unwrap();
        assert_eq!(candidates, vec!["small/a", "small/b"]);
        let matches = search_matches(&repo, &mut sc, &tree, "needleinsmall", &candidates).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "small/a");

        // Determinism across memoization states, coarse dirs included.
        let cold = MapCache::default();
        let index2 = trigram_index(&repo, &cold, &mut Indexer::default(), tree).unwrap();
        assert_eq!(index.id(), index2.id());
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
        let matches = search_matches(&repo, &mut sc, &tree_a, "zebra", &hits).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "sub1/gone");

        let hits = search_candidates(&repo, &mut sc, &index_b, &tree_b, "delta").unwrap();
        assert_eq!(hits, vec!["sub2/mod"]);
        let hits = search_candidates(&repo, &mut sc, &index_b, &tree_b, "zebra").unwrap();
        assert!(hits.is_empty());
        let hits = search_candidates(&repo, &mut sc, &index_b, &tree_b, "addition").unwrap();
        assert_eq!(hits, vec!["sub3/new"]);

        // Warm-cache results equal fresh-cache results.
        let fresh = search_candidates(
            &repo,
            &mut SearchCache::default(),
            &index_b,
            &tree_b,
            "delta",
        )
        .unwrap();
        assert_eq!(fresh, vec!["sub2/mod"]);
    }
}
