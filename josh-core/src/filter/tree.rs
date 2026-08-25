use super::*;
use crate::objects;
use anyhow::anyhow;

pub fn pathstree(
    root: &str,
    input: gix_hash::ObjectId,
    transaction: &cache::Transaction,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    pathstree_inner(root, input, transaction, odb)
}

/// Oid-level body of [`pathstree`]; the odb is hoisted like in [`remove_pred_inner`].
fn pathstree_inner(
    root: &str,
    input: gix_hash::ObjectId,
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
) -> anyhow::Result<gix_hash::ObjectId> {
    if let Some(cached) = transaction.get_paths((input, root.to_string())) {
        return Ok(cached);
    }

    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("pathstree: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());

    for entry in &tree.entries {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
        if entry.mode.is_tree() {
            let s = pathstree_inner(
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                entry.oid.to_owned(),
                transaction,
                odb,
            )?;

            if s != empty_id() {
                rebuild.keep(gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: entry.filename.to_owned(),
                    oid: s,
                });
            }
        } else if !entry.mode.is_commit() {
            // Blob-classified entries (including symlinks); gitlinks are skipped.
            let path = normalize_path(&Path::new(root).join(name));
            let path_string = path.to_str().ok_or_else(|| anyhow!("no name"))?;
            let file_contents = if name == "workspace.josh" {
                format!("#{}\n{}", path_string, blob_text(odb, entry.oid.to_owned()))
            } else {
                path_string.to_string()
            };
            rebuild.keep(gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Blob.into(),
                filename: entry.filename.to_owned(),
                oid: odb.write(gix_object::Kind::Blob, file_contents.as_bytes()),
            });
        }
    }
    let result = objects::write_tree_now(odb, rebuild.out)?;
    transaction.insert_paths((input, root.to_string()), result);
    Ok(result)
}

pub fn regex_replace(
    input: gix_hash::ObjectId,
    regex: &regex::Regex,
    replacement: &str,
    transaction: &cache::Transaction,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    regex_replace_inner(input, regex, replacement, transaction, odb)
}

/// Oid-level body of [`regex_replace`]; the odb is hoisted like in [`remove_pred_inner`].
fn regex_replace_inner(
    input: gix_hash::ObjectId,
    regex: &regex::Regex,
    replacement: &str,
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
) -> anyhow::Result<gix_hash::ObjectId> {
    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("regex_replace: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());

    for entry in &tree.entries {
        // Non-UTF-8 entry names stay an error even though the name is otherwise unused here.
        std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
        if entry.mode.is_tree() {
            let s =
                regex_replace_inner(entry.oid.to_owned(), regex, replacement, transaction, odb)?;

            if s != tree::empty_id() {
                rebuild.keep(gix_object::tree::Entry {
                    mode: entry.mode,
                    filename: entry.filename.to_owned(),
                    oid: s,
                });
            }
        } else if !entry.mode.is_commit() {
            let file_contents = blob_text(odb, entry.oid.to_owned());
            let replaced = regex.replacen(&file_contents, 0, replacement);

            rebuild.keep(gix_object::tree::Entry {
                mode: entry.mode,
                filename: entry.filename.to_owned(),
                oid: odb.write(gix_object::Kind::Blob, replaced.as_bytes()),
            });
        }
    }
    objects::write_tree_now(odb, rebuild.out)
}

/// The raw bytes of the blob `oid`, or `None` when the object is missing or not a blob --
/// `find_blob`'s tolerance, in facade currency.
pub fn blob_bytes(odb: &josh_memodb::Odb, oid: gix_hash::ObjectId) -> Option<josh_memodb::Bytes> {
    match odb.read(oid) {
        Ok((gix_object::Kind::Blob, bytes)) => Some(bytes),
        _ => None,
    }
}

/// The text content of the blob `oid`, or the empty string when the blob is missing, contains
/// a NUL byte, or is not valid UTF-8 -- the same tolerant semantics as [`get_blob`], minus the
/// path lookup. The NUL check keeps binary blobs from reading as content in workspace/link
/// parsing and text transforms.
pub(crate) fn blob_text(odb: &josh_memodb::Odb, oid: gix_hash::ObjectId) -> String {
    let bytes = some_or!(blob_bytes(odb, oid), {
        return "".to_owned();
    });
    if bytes.contains(&0) {
        return "".to_owned();
    }
    let content = ok_or!(std::str::from_utf8(&bytes), {
        return "".to_owned();
    });
    content.to_owned()
}

/// Compare two tree entry names in canonical git tree order: byte-wise, with tree entries
/// sorted as if their name had a trailing '/'.
fn git_tree_entry_cmp(a: &[u8], a_is_tree: bool, b: &[u8], b_is_tree: bool) -> std::cmp::Ordering {
    let len = a.len().min(b.len());
    match a[..len].cmp(&b[..len]) {
        std::cmp::Ordering::Equal => {}
        ord => return ord,
    }
    let ca = a
        .get(len)
        .copied()
        .unwrap_or(if a_is_tree { b'/' } else { 0 });
    let cb = b
        .get(len)
        .copied()
        .unwrap_or(if b_is_tree { b'/' } else { 0 });
    ca.cmp(&cb)
}

/// Accumulator for rebuilding a tree entry-by-entry. Callers report the fate of each input
/// entry (`keep` for survivors, `mark_changed` for anything dropped, rewritten or
/// renamed away) and `finish` writes the rebuilt tree or returns the input oid.
///
/// Rebuilds are byte-preserving: kept entries carry their raw modes (and mode spellings)
/// and stay in their original relative order, duplicates included -- duplicates share a
/// name, so a path predicate keeps or drops them together. A tree the filter keeps
/// entirely therefore round-trips byte-for-byte even when it is fsck-invalid (unsorted,
/// duplicate names, legacy modes): josh does not re-encode bytes the operation was never
/// asked to change.
struct TreeRebuild {
    out: Vec<gix_object::tree::Entry>,
    /// Whether the rebuilt entry set could serialize to anything other than the input
    /// byte-for-byte. While it stays false, `finish` returns the input oid without writing
    /// a tree; any dropped, rewritten or renamed-away entry sets it.
    changed: bool,
}

impl TreeRebuild {
    fn new(capacity: usize) -> Self {
        Self {
            out: Vec::with_capacity(capacity),
            changed: false,
        }
    }

    /// Record that the rebuilt tree differs from the input: an entry was dropped, rewritten
    /// or renamed away.
    fn mark_changed(&mut self) {
        self.changed = true;
    }

    /// Append a surviving entry, preserving the input's entry order.
    fn keep(&mut self, entry: gix_object::tree::Entry) {
        self.out.push(entry);
    }

    /// Write the rebuilt tree, or return `input` unchanged when nothing was dropped or
    /// rewritten: an untouched entry set reproduces `input` bit-identically, since git trees
    /// are content-addressed and the write preserves entry order.
    fn finish(
        self,
        odb: &josh_memodb::Odb,
        input: gix_hash::ObjectId,
    ) -> anyhow::Result<gix_hash::ObjectId> {
        if self.changed {
            objects::write_tree_now(odb, self.out)
        } else {
            #[cfg(debug_assertions)]
            debug_assert_eq!(objects::write_tree_now(odb, self.out)?, input);
            Ok(input)
        }
    }
}

/// Clone a parsed tree's entries for use as the starting set of a rebuild: names, raw modes
/// and entry order are taken over unvalidated and unnormalized, duplicates included.
fn seed_entries(tree: &gix_object::TreeRef) -> Vec<gix_object::tree::Entry> {
    tree.entries.iter().map(|e| (*e).into()).collect()
}

/// Whether the entries are in strictly increasing canonical git order (no duplicates).
/// Bisection lookups are only valid on such trees; [`lookup_entry`] takes the answer as a
/// flag so it is computed once per parsed tree, not per lookup.
fn entries_canonically_sorted(tree: &gix_object::TreeRef) -> bool {
    tree.entries.windows(2).all(|w| {
        git_tree_entry_cmp(
            w[0].filename,
            w[0].mode.is_tree(),
            w[1].filename,
            w[1].mode.is_tree(),
        ) == std::cmp::Ordering::Less
    })
}

/// Look up an entry by name irrespective of its kind, like git2's `Tree::get_name`.
/// Canonical order sorts a blob before a same-named tree, so the blob probe comes first.
/// Non-canonical trees (`sorted == false`) cannot be bisected and use a linear scan
/// instead, first occurrence wins; canonical trees -- the hot path -- never scan.
fn lookup_entry<'a>(
    tree: &gix_object::TreeRef<'a>,
    name: &gix_object::bstr::BStr,
    sorted: bool,
) -> Option<gix_object::tree::EntryRef<'a>> {
    if sorted {
        tree.bisect_entry(name, false)
            .or_else(|| tree.bisect_entry(name, true))
    } else {
        tree.entries.iter().find(|e| e.filename == name).copied()
    }
}

/// A tree held by its raw bytes, so it can be returned from [`read_tree`] and kept across a
/// whole filter arm. Lookups walk the entries lazily and allocate nothing, which suits the
/// sites that probe a handful of paths in one tree (a path descent, the workspace files of a
/// commit); [`ParsedTree`] is the counterpart for iterating every entry.
pub struct TreeReader {
    bytes: cache::TreeBytes,
}

impl TreeReader {
    /// Every entry, in stored tree order. Malformed entries are skipped.
    pub fn entries(&self) -> impl Iterator<Item = gix_object::tree::EntryRef<'_>> {
        gix_object::TreeRefIter::from_bytes(&self.bytes, gix_hash::Kind::Sha1)
            .filter_map(Result::ok)
    }

    /// Entry by name irrespective of kind, first occurrence winning -- tree entry order is
    /// not necessarily canonical, and a name is unique in a valid tree.
    pub fn entry(&self, name: &[u8]) -> Option<gix_object::tree::EntryRef<'_>> {
        gix_object::TreeRefIter::from_bytes(&self.bytes, gix_hash::Kind::Sha1)
            .filter_map(Result::ok)
            .find(|entry| entry.filename == name)
    }
}

/// Read the tree `oid`, erroring when the object is missing or not a tree.
pub fn read_tree(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<TreeReader> {
    let bytes = transaction
        .read_tree_bytes(odb, oid)?
        .ok_or_else(|| anyhow!("{} is not a tree", oid))?;
    Ok(TreeReader { bytes })
}

/// A parsed tree with its canonical-order flag computed once, so sites that iterate every
/// entry can also bisect for names. Borrows a caller-held buffer.
pub(crate) struct ParsedTree<'b> {
    tree: gix_object::TreeRef<'b>,
    sorted: bool,
}

impl<'b> ParsedTree<'b> {
    pub(crate) fn from_bytes(bytes: &'b [u8]) -> anyhow::Result<Self> {
        let tree = gix_object::TreeRef::from_bytes(bytes, gix_hash::Kind::Sha1)?;
        let sorted = entries_canonically_sorted(&tree);
        Ok(ParsedTree { tree, sorted })
    }

    /// Entry by name irrespective of kind (see [`lookup_entry`]).
    pub(crate) fn entry(&self, name: &[u8]) -> Option<gix_object::tree::EntryRef<'b>> {
        lookup_entry(&self.tree, name.into(), self.sorted)
    }

    pub(crate) fn entries(&self) -> &[gix_object::tree::EntryRef<'b>] {
        &self.tree.entries
    }
}

/// The normal components of `path`, or `None` for path shapes that can never name a tree
/// entry (empty, absolute, or containing `..`) -- rejected paths read as lookup misses.
/// `.` components and redundant separators normalize away.
fn path_components(path: &Path) -> Option<Vec<&[u8]>> {
    let mut components = vec![];
    for c in path.components() {
        match c {
            std::path::Component::Normal(name) => components.push(component_bytes(name)),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    if components.is_empty() {
        return None;
    }
    Some(components)
}

/// Descend `path` from the tree `root`. `Ok(None)` covers a missing component and a non-tree
/// intermediate entry; the final entry may be any kind (gitlinks included). Per level the
/// bytes come from `read_tree_bytes` (memory zero-copy / TreeCache), so unflushed trees
/// resolve.
pub fn get_path_entry(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    root: gix_hash::ObjectId,
    path: &Path,
) -> anyhow::Result<Option<gix_object::tree::Entry>> {
    let bytes = match transaction.read_tree_bytes(odb, root)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    get_path_entry_at(transaction, odb, &(TreeReader { bytes }), path)
}

/// [`get_path_entry`] resolving the first component against a caller-held parse -- the hoist
/// for multi-probe sites, which pay one root parse for N descents.
pub fn get_path_entry_at(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    root: &TreeReader,
    path: &Path,
) -> anyhow::Result<Option<gix_object::tree::Entry>> {
    let Some(components) = path_components(path) else {
        return Ok(None);
    };
    let mut entry: gix_object::tree::Entry = match root.entry(components[0]) {
        Some(entry) => entry.into(),
        None => return Ok(None),
    };
    for name in &components[1..] {
        if !entry.mode.is_tree() {
            return Ok(None);
        }
        let bytes = match transaction.read_tree_bytes(odb, entry.oid)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };
        entry = match (TreeReader { bytes }).entry(name) {
            Some(entry) => entry.into(),
            None => return Ok(None),
        };
    }
    Ok(Some(entry))
}

/// Insert `entry` at its canonical position in `out`, scanning back from the end (O(1) for
/// the common append-in-canonical-order case). In a non-canonical entry set the position is
/// best-effort: the entry lands after the last existing entry that canonically precedes it,
/// and the existing order is left untouched.
fn insert_in_order(out: &mut Vec<gix_object::tree::Entry>, entry: gix_object::tree::Entry) {
    let is_tree = entry.mode.is_tree();
    let mut pos = out.len();
    while pos > 0
        && git_tree_entry_cmp(
            &out[pos - 1].filename,
            out[pos - 1].mode.is_tree(),
            &entry.filename,
            is_tree,
        ) == std::cmp::Ordering::Greater
    {
        pos -= 1;
    }
    out.insert(pos, entry);
}

/// Rebuild `input` keeping only blob entries accepted by `pred`. `path` is a reusable buffer
/// holding the slash-separated path of the tree currently being visited; it is restored to its
/// incoming length before returning. Gitlink (submodule) entries are always dropped.
///
/// The predicate sees full paths, so the result of a subtree depends on where it sits, not only
/// on its oid. The cache key therefore folds the current root path into a synthetic oid (the
/// insert_invert idiom): identical subtrees at different paths get distinct entries.
pub fn remove_pred(
    transaction: &cache::Transaction,
    path: &mut String,
    input: gix_hash::ObjectId,
    pred: &dyn Fn(&str, bool) -> bool,
    key: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    remove_pred_inner(transaction, odb, path, input, pred, key)
}

/// Recursive body of [`remove_pred`], with the odb handle created once at the entry point --
/// creating one per recursion level costs an FFI round-trip per tree node.
fn remove_pred_inner(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    path: &mut String,
    input: gix_hash::ObjectId,
    pred: &dyn Fn(&str, bool) -> bool,
    key: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let root_key = objects::hash_blob(format!("glob-fallback:{:?}:{}", key, path).as_bytes());
    if let Some(cached) = transaction.get_glob((input, root_key, 0)) {
        return Ok(cached);
    }

    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("remove_pred: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());
    let empty = empty_id();

    for entry in &tree.entries {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("INVALID_FILENAME"))?;
        let base = path.len();
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(name);

        if entry.mode.is_tree() {
            let s = remove_pred_inner(transaction, odb, path, entry.oid.to_owned(), pred, key)?;
            if s != entry.oid.to_owned() || s == empty {
                rebuild.mark_changed();
            }
            if s != empty {
                rebuild.keep(gix_object::tree::Entry {
                    mode: entry.mode,
                    filename: entry.filename.to_owned(),
                    oid: s,
                });
            }
        } else if entry.mode.is_commit() {
            // Gitlinks are dropped, so the rebuilt tree differs from the input and must not
            // take the unchanged fast path.
            rebuild.mark_changed();
        } else if pred(path, true) {
            // Kept verbatim: raw mode (and mode spelling) included.
            rebuild.keep((*entry).into());
        } else {
            rebuild.mark_changed();
        }
        // Rewind the shared buffer to the parent path: `base` is its length from before this
        // entry's ('/' + ) name was appended, so one reused buffer replaces a per-entry PathBuf
        // allocation. Skipped on error returns above, which abort the whole walk anyway.
        path.truncate(base);
    }

    let result = rebuild.finish(odb, input)?;
    transaction.insert_glob((input, root_key, 0), result);
    Ok(result)
}

pub use josh_filter::pattern::{CompiledPattern, PATTERN_MATCH_OPTIONS, PatternComponent};

/// Component-wise NFA walk for `Op::Pattern`: rebuild `input` keeping exactly the blobs whose
/// full path matches the pattern, without ever materializing full paths. `state` is a bitmask of
/// active component positions (bit `p` set = components `0..p` already consumed by ancestor
/// directories). Subtrees whose next state is empty cannot contain any match and are pruned
/// without recursion.
///
/// Results are cached by `(input, key, closed state)` -- `key` identifies the pattern (the
/// peeled Pattern filter's id): the closed state fully determines match behavior below a
/// subtree independent of the path taken to reach it, so identical subtrees reached in the same
/// state share one entry, while identical subtrees in different states (e.g. inside vs outside
/// a literal prefix) stay separate.
pub fn remove_pattern(
    transaction: &cache::Transaction,
    input: gix_hash::ObjectId,
    cp: &CompiledPattern,
    key: gix_hash::ObjectId,
    state: u64,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    remove_pattern_inner(transaction, odb, input, cp, key, state)
}

/// Recursive body of [`remove_pattern`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn remove_pattern_inner(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    input: gix_hash::ObjectId,
    cp: &CompiledPattern,
    key: gix_hash::ObjectId,
    state: u64,
) -> anyhow::Result<gix_hash::ObjectId> {
    let state = cp.closure(state);
    if let Some(cached) = transaction.get_glob((input, key, state)) {
        return Ok(cached);
    }

    let k = cp.components.len();
    // Whether any active position can accept a blob at this level: a non-`**` final component, or
    // a `**` with only `**`s after it (which matches any non-dot suffix of components).
    let mut accepts_blobs = false;
    {
        let mut bits = state;
        while bits != 0 {
            let p = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            accepts_blobs |= match cp.components[p] {
                PatternComponent::Glob(_) => p == k - 1,
                PatternComponent::Star2 => cp.suffix_all_star[p],
            };
        }
    }

    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("remove_pattern: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());
    let empty = empty_id();

    for entry in &tree.entries {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("INVALID_FILENAME"))?;

        if entry.mode.is_tree() {
            // Successor state: a matching non-final glob component advances to `p + 1`; a
            // `**` also stays at `p` for any non-dot name (its zero-width advance is part of
            // the closure).
            let mut next = 0u64;
            let mut bits = state;
            while bits != 0 {
                let p = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                match &cp.components[p] {
                    PatternComponent::Glob(g) => {
                        if p + 1 < k && g.matches_with(name, PATTERN_MATCH_OPTIONS) {
                            next |= 1 << (p + 1);
                        }
                    }
                    PatternComponent::Star2 => {
                        if !name.starts_with('.') {
                            next |= 1 << p;
                        }
                    }
                }
            }
            // An empty successor state can match nothing below this subtree: prune it
            // entirely, identical to the full walk producing an empty result that is then
            // omitted.
            let s = if next == 0 {
                empty
            } else {
                remove_pattern_inner(transaction, odb, entry.oid.to_owned(), cp, key, next)?
            };
            if s != entry.oid.to_owned() || s == empty {
                rebuild.mark_changed();
            }
            if s != empty {
                rebuild.keep(gix_object::tree::Entry {
                    mode: entry.mode,
                    filename: entry.filename.to_owned(),
                    oid: s,
                });
            }
        } else if entry.mode.is_commit() {
            // Gitlinks are dropped, as in remove_pred.
            rebuild.mark_changed();
        } else {
            let mut keep = false;
            if accepts_blobs {
                let mut bits = state;
                while bits != 0 && !keep {
                    let p = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    keep = match &cp.components[p] {
                        PatternComponent::Glob(g) => {
                            p == k - 1 && g.matches_with(name, PATTERN_MATCH_OPTIONS)
                        }
                        PatternComponent::Star2 => cp.suffix_all_star[p] && !name.starts_with('.'),
                    };
                }
            }
            if keep {
                // Kept verbatim, as in remove_pred.
                rebuild.keep((*entry).into());
            } else {
                rebuild.mark_changed();
            }
        }
    }

    // See remove_pred: an untouched entry set reproduces `input` bit-identically.
    let result = rebuild.finish(odb, input)?;
    transaction.insert_glob((input, key, state), result);
    Ok(result)
}

pub fn subtract(
    transaction: &cache::Transaction,
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    subtract_inner(transaction, odb, input1, input2)
}

/// Recursive body of [`subtract`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn subtract_inner(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if input1 == input2 {
        return Ok(empty_id());
    }
    if input1 == empty_id() {
        return Ok(empty_id());
    }

    if let Some(cached) = transaction.get_subtract((input1, input2)) {
        return Ok(cached);
    }

    let bytes1 = transaction.read_tree_bytes(odb, input1)?;
    let bytes2 = transaction.read_tree_bytes(odb, input2)?;
    if let (Some(bytes1), Some(bytes2)) = (bytes1, bytes2) {
        if input2 == empty_id() {
            return Ok(input1);
        }
        let tree1 = gix_object::TreeRef::from_bytes(&bytes1, gix_hash::Kind::Sha1)?;
        let tree2 = gix_object::TreeRef::from_bytes(&bytes2, gix_hash::Kind::Sha1)?;
        let sorted1 = entries_canonically_sorted(&tree1);
        // Start from `tree1` and drop or replace each path that also appears in `tree2`.
        // Modifications are collected by name first and applied in one pass: `None` removes
        // the entry, `Some` replaces its oid with the subtraction result (only ever produced
        // for a tree entry, whose raw mode is kept).
        let mut mods: std::collections::HashMap<&[u8], Option<gix_hash::ObjectId>> =
            std::collections::HashMap::new();
        for entry in &tree2.entries {
            if let Some(e1) = lookup_entry(&tree1, entry.filename, sorted1) {
                let sub =
                    subtract_inner(transaction, odb, e1.oid.to_owned(), entry.oid.to_owned())?;
                if sub == empty_id() || sub == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    mods.insert(&**entry.filename, None);
                } else {
                    mods.insert(&**entry.filename, Some(sub));
                }
            }
        }

        let mut out = seed_entries(&tree1);
        out.retain(|e| mods.get(e.filename.as_slice()) != Some(&None));
        for entry in &mut out {
            if let Some(Some(sub)) = mods.get(entry.filename.as_slice()) {
                entry.oid = *sub;
            }
        }
        let result = objects::write_tree_now(odb, out)?;

        transaction.insert_subtract((input1, input2), result);

        return Ok(result);
    }

    transaction.insert_subtract((input1, input2), empty_id());

    Ok(empty_id())
}

/// Intersect two trees by path: keep every entry of `input1` whose path also exists in `input2`,
/// carrying `input1`'s content and mode. This is the exact complement of [`subtract`] over `input1`
/// -- `subtract` drops the shared paths, `intersect` keeps them -- so
/// `intersect(a, b) == subtract(a, subtract(a, b))`. Computing it directly (rather than via that
/// double subtract) matters for performance: the double subtract's outer step iterates `a`'s
/// complement, which is nearly all of `a`, whereas this iterates only `input2`. Selecting a small
/// set of paths out of a large tree therefore costs O(input2) instead of O(input1).
pub fn intersect(
    transaction: &cache::Transaction,
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    intersect_inner(transaction, odb, input1, input2)
}

/// Recursive body of [`intersect`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn intersect_inner(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    // Identical (sub)trees intersect to themselves; an empty side leaves nothing to keep.
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == empty_id() || input2 == empty_id() {
        return Ok(empty_id());
    }

    if let Some(cached) = transaction.get_intersect((input1, input2)) {
        return Ok(cached);
    }

    let bytes1 = transaction.read_tree_bytes(odb, input1)?;
    let bytes2 = transaction.read_tree_bytes(odb, input2)?;
    let result = if let (Some(bytes1), Some(bytes2)) = (bytes1, bytes2) {
        let tree1 = gix_object::TreeRef::from_bytes(&bytes1, gix_hash::Kind::Sha1)?;
        let tree2 = gix_object::TreeRef::from_bytes(&bytes2, gix_hash::Kind::Sha1)?;
        let sorted1 = entries_canonically_sorted(&tree1);
        // Iterate the selector (`input2`), keeping each of its paths that also exists in `tree1`
        // with `tree1`'s content and raw mode; cost tracks the size of the selected set.
        let mut out = Vec::new();
        for entry in &tree2.entries {
            if let Some(e1) = lookup_entry(&tree1, entry.filename, sorted1) {
                let child = intersect(transaction, e1.oid.to_owned(), entry.oid.to_owned())?;
                if child != empty_id() && child != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    insert_in_order(
                        &mut out,
                        gix_object::tree::Entry {
                            mode: e1.mode,
                            filename: entry.filename.to_owned(),
                            oid: child,
                        },
                    );
                }
            }
        }
        objects::write_tree_now(odb, out)?
    } else {
        // At least one side is a blob at this already-name-matched path, so the path exists in both;
        // keep `input1`'s content, matching the path-based semantics of the tree case.
        input1
    };

    transaction.insert_intersect((input1, input2), result);

    Ok(result)
}

/// The raw bytes of a path component, for matching against tree entry names.
fn component_bytes(c: &std::ffi::OsStr) -> &[u8] {
    josh_gix_ext::component_bytes(c)
}

/// Read `oid` as raw tree bytes, or `None` if it is missing or not a tree. Uncached: the
/// insert path can be called without a transaction to hang the tree cache off.
fn tree_bytes(src: &impl gix_object::Find, oid: gix_hash::ObjectId) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    match src.try_find(&oid, &mut buf) {
        Ok(Some(data)) if data.kind == gix_object::Kind::Tree => Some(buf),
        _ => None,
    }
}

/// Rebuild the tree `tree_oid` with the single-component `child` replaced by (`oid`, `mode`), or
/// removed when `oid` is the zero or empty-tree oid. Other entries keep their raw modes and
/// order; a new entry lands at its canonical position. This op defines the single entry for
/// `child`, so duplicates of the name collapse: every occurrence is removed and the
/// replacement takes the first one's position.
fn replace_child_inner(
    odb: &(impl gix_object::Find + gix_object::Write),
    child: &[u8],
    oid: gix_hash::ObjectId,
    mode: i32,
    tree_oid: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut out = match tree_bytes(odb, tree_oid) {
        Some(bytes) => seed_entries(&gix_object::TreeRef::from_bytes(
            &bytes,
            gix_hash::Kind::Sha1,
        )?),
        None => Vec::new(),
    };
    let remove = oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) || oid == empty_id();
    let first = out.iter().position(|e| &*e.filename == child);
    out.retain(|e| &*e.filename != child);
    if !remove {
        let entry = gix_object::tree::Entry {
            mode: gix_object::tree::EntryMode::try_from(mode as u32)
                .map_err(|m| anyhow!("replace_child: invalid mode {:o}", m))?,
            filename: child.into(),
            oid: oid,
        };
        match first {
            Some(pos) => out.insert(pos, entry),
            None => insert_in_order(&mut out, entry),
        }
    }
    objects::write_tree_now(odb, out)
}

/// Oid-level body of [`insert`]: replace whatever is at `path` inside the tree `full_tree` with
/// (`oid`, `mode`), creating intermediate trees as needed and treating blobs on the way as
/// overwritable.
pub fn insert_oid(
    odb: &(impl gix_object::Find + gix_object::Write),
    full_tree: gix_hash::ObjectId,
    path: &Path,
    oid: gix_hash::ObjectId,
    mode: i32,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut components = path.components();
    let Some(first) = components.next() else {
        return Err(anyhow!("file_name"));
    };
    if components.next().is_none() {
        return replace_child_inner(
            odb,
            component_bytes(first.as_os_str()),
            oid,
            mode,
            full_tree,
        );
    }
    let name = path.file_name().ok_or_else(|| anyhow!("file_name"))?;
    let parent = path.parent().ok_or_else(|| anyhow!("path.parent"))?;

    // The subtree at `parent`, or the empty tree when the path is missing or passes through a
    // non-tree entry.
    let mut st = full_tree;
    for c in parent.components() {
        let cb = component_bytes(c.as_os_str());
        st = match tree_bytes(odb, st) {
            Some(bytes) => {
                let tree = gix_object::TreeRef::from_bytes(&bytes, gix_hash::Kind::Sha1)?;
                let sorted = entries_canonically_sorted(&tree);
                match lookup_entry(&tree, cb.into(), sorted) {
                    Some(e) => e.oid.to_owned(),
                    None => empty_id(),
                }
            }
            None => empty_id(),
        };
    }

    let subtree = replace_child_inner(odb, component_bytes(name), oid, mode, st)?;

    insert_oid(odb, full_tree, parent, subtree, 0o0040000)
}

/// Kind of `oid`, or `None` when the object is missing (the zero oid included) or the
/// header unreadable -- both read as an ordinary miss.
fn kind_of(src: &impl gix_object::FindHeader, oid: gix_hash::ObjectId) -> Option<gix_object::Kind> {
    src.try_header(&oid).ok().flatten().map(|h| h.kind)
}

/// Fill `buf` with the raw bytes of `oid` if it is a readable tree object. Parse failures
/// fold to `None` at the caller, keeping the probe arms tolerant of corrupt trees.
fn read_tree_into(src: &impl gix_object::Find, oid: gix_hash::ObjectId, buf: &mut Vec<u8>) -> bool {
    matches!(
        src.try_find(&oid, buf),
        Ok(Some(data)) if data.kind == gix_object::Kind::Tree
    )
}

pub fn diff_paths(
    src: &(impl gix_object::Find + gix_object::FindHeader),
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
    root: &str,
) -> anyhow::Result<Vec<(String, i32)>> {
    if input1 == input2 {
        return Ok(vec![]);
    }

    use gix_object::Kind;
    let kind1 = kind_of(src, input1);
    let kind2 = kind_of(src, input2);

    if let (Some(Kind::Blob), Some(Kind::Blob)) = (kind1, kind2) {
        return Ok(vec![(root.to_string(), 0)]);
    }

    if let (Some(Kind::Blob), _) = (kind1, kind2) {
        return Ok(vec![(root.to_string(), -1)]);
    }

    if let (_, Some(Kind::Blob)) = (kind1, kind2) {
        return Ok(vec![(root.to_string(), 1)]);
    }

    let mut r = vec![];

    let mut buf1 = Vec::new();
    let mut buf2 = Vec::new();
    let tree1 = read_tree_into(src, input1, &mut buf1)
        .then(|| ParsedTree::from_bytes(&buf1).ok())
        .flatten();
    let tree2 = read_tree_into(src, input2, &mut buf2)
        .then(|| ParsedTree::from_bytes(&buf2).ok())
        .flatten();

    if let (Some(tree1), Some(tree2)) = (&tree1, &tree2) {
        for entry in tree2.entries() {
            let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
            if let Some(e) = tree1.entry(entry.filename) {
                r.append(&mut diff_paths(
                    src,
                    e.oid.to_owned(),
                    entry.oid.to_owned(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            } else {
                r.append(&mut diff_paths(
                    src,
                    gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    entry.oid.to_owned(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        for entry in tree1.entries() {
            let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
            if tree2.entry(entry.filename).is_none() {
                r.append(&mut diff_paths(
                    src,
                    entry.oid.to_owned(),
                    gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        return Ok(r);
    }

    if let Some(tree2) = &tree2 {
        for entry in tree2.entries() {
            let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
            r.append(&mut diff_paths(
                src,
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                entry.oid.to_owned(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    if let Some(tree1) = &tree1 {
        for entry in tree1.entries() {
            let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
            r.append(&mut diff_paths(
                src,
                entry.oid.to_owned(),
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    Ok(r)
}

pub fn overlay(
    transaction: &cache::Transaction,
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    overlay_inner(transaction, odb, input1, input2)
}

/// Recursive body of [`overlay`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn overlay_inner(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    input1: gix_hash::ObjectId,
    input2: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if let Some(cached) = transaction.get_overlay((input1, input2)) {
        return Ok(cached);
    }
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == empty_id() {
        return Ok(input2);
    }
    if input2 == empty_id() {
        return Ok(input1);
    }

    let bytes1 = transaction.read_tree_bytes(odb, input1)?;
    let bytes2 = transaction.read_tree_bytes(odb, input2)?;
    if let (Some(bytes1), Some(bytes2)) = (bytes1, bytes2) {
        let tree1 = gix_object::TreeRef::from_bytes(&bytes1, gix_hash::Kind::Sha1)?;
        let tree2 = gix_object::TreeRef::from_bytes(&bytes2, gix_hash::Kind::Sha1)?;
        let sorted1 = entries_canonically_sorted(&tree1);
        // Start from `tree1` and insert every entry of `tree2`: recursively overlaid where the
        // name exists in `tree1` (with `tree1` winning on blob collisions, keeping its raw
        // mode), taken over as-is otherwise -- placed at its canonical position, raw mode
        // included.
        let mut mods: std::collections::HashMap<&[u8], gix_hash::ObjectId> =
            std::collections::HashMap::new();
        let mut new_entries: Vec<gix_object::tree::Entry> = Vec::new();
        for entry in &tree2.entries {
            if let Some(e1) = lookup_entry(&tree1, entry.filename, sorted1) {
                let id = overlay_inner(transaction, odb, e1.oid.to_owned(), entry.oid.to_owned())?;
                mods.insert(&**entry.filename, id);
            } else {
                new_entries.push((*entry).into());
            }
        }

        let mut out = seed_entries(&tree1);
        for entry in &mut out {
            if let Some(id) = mods.get(entry.filename.as_slice()) {
                entry.oid = *id;
            }
        }
        for entry in new_entries {
            insert_in_order(&mut out, entry);
        }

        let rid = objects::write_tree_now(odb, out)?;

        transaction.insert_overlay((input1, input2), rid);
        return Ok(rid);
    }

    Ok(input1)
}

pub fn pathline(b: &str) -> anyhow::Result<String> {
    match b
        .split('\n')
        .next()
        .map(|line| line.trim_start_matches('#'))
    {
        Some(line) if !line.is_empty() => Ok(line.to_string()),
        Some(_) | None => Err(anyhow!("pathline")),
    }
}

pub fn invert_paths(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    root: &str,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if let Some(cached) = transaction.get_invert((tree, root.to_string())) {
        return Ok(cached);
    }

    let mut result = empty_id();

    let bytes = transaction
        .read_tree_bytes(odb, tree)?
        .ok_or_else(|| anyhow!("invert_paths: {} is not a tree", tree))?;
    let parsed = ParsedTree::from_bytes(&bytes)?;

    for entry in parsed.entries() {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;

        if !entry.mode.is_tree() && !entry.mode.is_commit() {
            let mpath = normalize_path(&Path::new(root).join(name))
                .to_string_lossy()
                .to_string();
            // Same one-level lookup get_blob did (first-match by name, not this iteration's
            // entry) -- resolved on the hoisted parse.
            let b = parsed
                .entry(entry.filename)
                .map(|e| blob_text(odb, e.oid.to_owned()))
                .unwrap_or_default();
            let opath = pathline(&b)?;

            result = insert_oid(
                odb,
                result,
                Path::new(&opath),
                odb.write(gix_object::Kind::Blob, mpath.as_bytes()),
                0o0100644,
            )
            .unwrap();
        }

        if entry.mode.is_tree() {
            let s = invert_paths(
                transaction,
                odb,
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                entry.oid.to_owned(),
            )?;
            result = overlay(transaction, result, s)?;
        }
    }

    transaction.insert_invert((tree, root.to_string()), result);

    Ok(result)
}

pub fn original_path(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: gix_hash::ObjectId,
    path: &Path,
) -> anyhow::Result<String> {
    let paths_tree = apply(
        transaction,
        to_filter(Op::Paths).chain(filter),
        Rewrite::from_tree(tree),
    )?;
    let b = get_blob(transaction, transaction.odb(), paths_tree.tree_id(), path);
    pathline(&b)
}

pub fn repopulated_tree(
    transaction: &cache::Transaction,
    filter: Filter,
    full_tree: gix_hash::ObjectId,
    partial_tree: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let paths_tree = apply(
        transaction,
        to_filter(Op::Paths).chain(filter),
        Rewrite::from_tree(full_tree),
    )?;

    let odb = transaction.odb();
    let ipaths = invert_paths(transaction, odb, "", paths_tree.tree_id())?;
    populate(transaction, odb, ipaths, partial_tree)
}

pub fn populate(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    paths: gix_hash::ObjectId,
    content: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    if let Some(cached) = transaction.get_populate((paths, content)) {
        return Ok(cached);
    }

    use gix_object::Kind;
    let paths_kind = odb.read_header(paths).map(|(kind, _)| kind).ok();
    let content_kind = odb.read_header(content).map(|(kind, _)| kind).ok();

    let mut result_tree = empty_id();
    if let (Some(Kind::Blob), Some(Kind::Blob)) = (paths_kind, content_kind) {
        let paths_bytes = blob_bytes(odb, paths).ok_or_else(|| anyhow!("populate: blob read"))?;
        let ipath = pathline(std::str::from_utf8(&paths_bytes)?)?;
        result_tree = insert_oid(odb, result_tree, Path::new(&ipath), content, 0o0100644)?;
    } else if let (Some(Kind::Tree), Some(Kind::Tree)) = (paths_kind, content_kind) {
        let paths_bytes = transaction
            .read_tree_bytes(odb, paths)?
            .ok_or_else(|| anyhow!("populate: {} is not a tree", paths))?;
        let content_bytes = transaction
            .read_tree_bytes(odb, content)?
            .ok_or_else(|| anyhow!("populate: {} is not a tree", content))?;
        let paths_tree = ParsedTree::from_bytes(&paths_bytes)?;
        let content_tree = ParsedTree::from_bytes(&content_bytes)?;
        for entry in content_tree.entries() {
            std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
            if let Some(e) = paths_tree.entry(entry.filename) {
                result_tree = overlay(
                    transaction,
                    result_tree,
                    populate(transaction, odb, e.oid.to_owned(), entry.oid.to_owned())?,
                )?;
            }
        }
    }

    transaction.insert_populate((paths, content), result_tree);

    Ok(result_tree)
}

pub fn compose(
    transaction: &cache::Transaction,
    trees: Vec<(&Filter, gix_hash::ObjectId)>,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut result = empty_id();
    let mut taken = empty_id();
    for (f, applied) in trees {
        let tid = taken;
        // If a filter creates a tree entry that does not exist in the input (Like TreeId and Blob),
        // the "output uniqueness handling" will cause it's output entry to be removed from the
        // tree during compose.
        // Note that f is only used for uniqueness calculation in this function so normalizing
        // it using double invert is ok and and does not affect the output of the filter itself,
        // since the original filter was already applied by the caller and passed via the "trees"
        // parameter.
        let f = invert(invert(*f)?)?;
        let taken_applied = if let Some(cached) = transaction.get_apply(f, tid) {
            cached
        } else {
            apply(transaction, f, Rewrite::from_tree(taken))?.tree_id()
        };
        transaction.insert_apply(f, tid, taken_applied);

        let subtracted = subtract(transaction, applied, taken_applied)?;

        let aid = applied;
        let unapplied = if let Some(cached) = transaction.get_unapply(f, aid) {
            cached
        } else {
            apply(transaction, invert(f)?, Rewrite::from_tree(applied))?.tree_id()
        };
        transaction.insert_unapply(f, aid, unapplied);
        taken = overlay(transaction, taken, unapplied)?;
        result = overlay(transaction, subtracted, result)?;
    }

    Ok(result)
}

pub fn get_blob(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
    path: &Path,
) -> String {
    let entry = match get_path_entry(transaction, odb, tree, path) {
        Ok(Some(entry)) => entry,
        _ => return "".to_owned(),
    };
    blob_text(odb, entry.oid)
}

/// [`get_blob`] over a caller-held parse of the root tree (see [`get_path_entry_at`]).
pub(crate) fn get_blob_at(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    root: &TreeReader,
    path: &Path,
) -> String {
    let entry = match get_path_entry_at(transaction, odb, root, path) {
        Ok(Some(entry)) => entry,
        _ => return "".to_owned(),
    };
    blob_text(odb, entry.oid)
}

pub fn empty_id() -> gix_hash::ObjectId {
    gix_hash::ObjectId::empty_tree(gix_hash::Kind::Sha1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(repo: &gix::Repository, paths: &[&str]) -> gix_hash::ObjectId {
        let mut builder = repo.edit_tree(empty_id()).unwrap();
        for path in paths {
            let oid = josh_gix_ext::write_blob(&repo.objects, path.as_bytes()).unwrap();
            builder
                .upsert(*path, gix::objs::tree::EntryKind::Blob, oid)
                .unwrap();
        }
        builder.write().unwrap().detach()
    }

    fn open_transaction(td: &tempfile::TempDir) -> cache::Transaction {
        let cachestack =
            cache::CacheStack::new().with_backend(cache::SledCacheBackend::new(td.path()));
        let ctx = cache::TransactionContext::new(td.path(), cachestack.into());
        ctx.open().unwrap()
    }

    // A gitlink (submodule) entry must be dropped from the rebuilt tree -- and must therefore
    // defeat the "unchanged input" fast path -- while a symlink blob accepted by the predicate
    // keeps its 0o120000 filemode.
    #[test]
    fn remove_pred_drops_gitlink_and_preserves_symlink_mode() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let blob = josh_gix_ext::write_blob(&repo.objects, b"content").unwrap();
        let link = josh_gix_ext::write_blob(&repo.objects, b"target").unwrap();
        // Gitlinks reference commits in other repositories; git does not require the oid
        // to exist locally.
        let sub = gix_hash::ObjectId::from_str("0123456789012345678901234567890123456789").unwrap();

        let input = write_raw_tree(
            &repo,
            &[
                ("100644", "keep.rs", blob),
                ("120000", "link.rs", link),
                ("160000", "sub", sub),
            ],
        );

        let t = open_transaction(&td);
        let key = gix_hash::ObjectId::from_str("1111111111111111111111111111111111111111").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();

        assert_ne!(out, input, "dropping the gitlink must produce a new tree");
        let out_tree = out_entries(&t, out);
        assert!(
            out_entry(&out_tree, "sub").is_none(),
            "gitlink must be dropped"
        );
        assert!(out_entry(&out_tree, "keep.rs").is_some());
        let link_entry = out_entry(&out_tree, "link.rs").expect("symlink kept");
        assert_eq!(link_entry.mode.value(), 0o120000);
        assert_eq!(link_entry.oid, link);
    }

    // The predicate must see full slash-separated paths at every depth (truncate discipline of
    // the shared path buffer), and a keep-everything predicate must return the input oid via the
    // unchanged fast path.
    #[test]
    fn remove_pred_passes_full_paths_and_reuses_unchanged_input() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let paths = ["a/b/drop.txt", "a/b/keep.rs", "a/keep.rs", "top.rs"];
        let input = make_tree(&repo, &paths);

        let t = open_transaction(&td);
        let key = gix_hash::ObjectId::from_str("2222222222222222222222222222222222222222").unwrap();

        let seen = std::cell::RefCell::new(Vec::new());
        let pred = |path: &str, isblob: bool| {
            assert!(isblob, "predicate must only be called for blobs");
            seen.borrow_mut().push(path.to_string());
            path.ends_with(".rs")
        };
        let out = remove_pred(&t, &mut String::new(), input, &pred, key).unwrap();

        let mut seen = seen.into_inner();
        seen.sort();
        assert_eq!(seen, paths);

        let odb = t.odb();
        for kept in ["a/b/keep.rs", "a/keep.rs", "top.rs"] {
            assert!(
                objects::path_entry(odb, out, Path::new(kept))
                    .unwrap()
                    .is_some(),
                "{kept} kept"
            );
        }
        assert!(
            objects::path_entry(odb, out, Path::new("a/b/drop.txt"))
                .unwrap()
                .is_none()
        );

        let key2 =
            gix_hash::ObjectId::from_str("3333333333333333333333333333333333333333").unwrap();
        let out2 = remove_pred(&t, &mut String::new(), input, &|_, _| true, key2).unwrap();
        assert_eq!(out2, input, "keep-everything must return the input oid");
    }

    /// Read a tree the code under test produced: its result lives in the transaction's store,
    /// so it is read through the facade rather than the repository handle.
    fn out_entries(
        t: &cache::Transaction,
        oid: gix_hash::ObjectId,
    ) -> Vec<gix_object::tree::Entry> {
        objects::read_tree_entries(t.odb(), oid).unwrap()
    }

    fn out_entry(
        entries: &[gix_object::tree::Entry],
        name: &str,
    ) -> Option<gix_object::tree::Entry> {
        entries
            .iter()
            .find(|e| e.filename == name.as_bytes())
            .cloned()
    }

    // Write a raw (unvalidated) tree object straight into the odb. This can express fsck-invalid
    // trees -- legacy filemodes, unsorted or duplicate entries, forbidden names -- that git can
    // still transport with default settings and that therefore reach remove_pred in production.
    fn write_raw_tree(
        repo: &gix::Repository,
        entries: &[(&str, &str, gix_hash::ObjectId)],
    ) -> gix_hash::ObjectId {
        let mut data = Vec::new();
        for (mode, name, oid) in entries {
            data.extend_from_slice(mode.as_bytes());
            data.push(b' ');
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(oid.as_bytes());
        }
        gix_object::Write::write_buf(&repo.objects, gix_object::Kind::Tree, &data).unwrap()
    }

    // A tree the filter keeps entirely round-trips byte-for-byte, no matter how fsck-invalid
    // it is: legacy blob modes, the "040000" tree-mode spelling, non-canonical entry order and
    // duplicate names all pass through verbatim via the unchanged fast path.
    #[test]
    fn remove_pred_preserves_fsck_invalid_trees() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let blob = josh_gix_ext::write_blob(&repo.objects, b"content").unwrap();
        let blob2 = josh_gix_ext::write_blob(&repo.objects, b"other").unwrap();
        let t = open_transaction(&td);

        let sub = write_raw_tree(&repo, &[("100644", "inner.rs", blob)]);
        let input = write_raw_tree(
            &repo,
            &[
                ("100664", "legacy.rs", blob),
                ("040000", "dir.rs", sub),
                ("100644", "b.rs", blob),
                ("100644", "a.rs", blob),
                ("100644", "a.rs", blob2),
            ],
        );
        let key = gix_hash::ObjectId::from_str("4444444444444444444444444444444444444444").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, _| true, key).unwrap();
        assert_eq!(out, input, "kept-entirely trees pass through verbatim");
    }

    // A rebuild forced by dropping entries keeps the survivors' raw modes, spellings and
    // relative order; duplicate names share their fate by name.
    #[test]
    fn remove_pred_rebuild_preserves_survivors() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let blob = josh_gix_ext::write_blob(&repo.objects, b"content").unwrap();
        let blob2 = josh_gix_ext::write_blob(&repo.objects, b"other").unwrap();
        let t = open_transaction(&td);

        let input = write_raw_tree(
            &repo,
            &[
                ("100664", "legacy.rs", blob),
                ("100644", "drop.txt", blob),
                ("100644", "b.rs", blob),
                ("100644", "a.rs", blob),
                ("100644", "a.rs", blob2),
            ],
        );
        let expected = write_raw_tree(
            &repo,
            &[
                ("100664", "legacy.rs", blob),
                ("100644", "b.rs", blob),
                ("100644", "a.rs", blob),
                ("100644", "a.rs", blob2),
            ],
        );
        let key = gix_hash::ObjectId::from_str("5555555555555555555555555555555555555555").unwrap();
        let out = remove_pred(
            &t,
            &mut String::new(),
            input,
            &|p, _| p.ends_with(".rs"),
            key,
        )
        .unwrap();
        assert_eq!(
            out, expected,
            "survivors keep raw modes and original order, duplicates included"
        );
    }

    // Entry names are never validated: a ".git" entry reads, filters and inserts like any
    // other name. Protecting checkouts from such names is the git client's job
    // (core.protectNTFS/core.protectHFS), not josh's.
    #[test]
    fn protected_names_are_not_special() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let blob = josh_gix_ext::write_blob(&repo.objects, b"content").unwrap();
        let t = open_transaction(&td);

        let input = write_raw_tree(
            &repo,
            &[("100644", ".git", blob), ("100644", "keep.rs", blob)],
        );
        let key = gix_hash::ObjectId::from_str("6666666666666666666666666666666666666666").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(out, input, ".git in input passes through verbatim");

        let odb = t.odb();
        let inserted = insert_oid(odb, input, Path::new("sub/.git"), blob, 0o100644).unwrap();
        assert_eq!(
            get_path_entry(&t, odb, inserted, Path::new("sub/.git"))
                .unwrap()
                .map(|e| e.oid),
            Some(blob),
            "inserted names are written as given"
        );
    }

    // subtract and overlay must resolve names in non-canonical trees via the linear lookup
    // fallback -- bisection would miss entries that sit at the wrong position -- and place
    // newly inserted entries at their canonical position.
    #[test]
    fn subtract_overlay_tolerate_non_canonical_order() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let blob = josh_gix_ext::write_blob(&repo.objects, b"content").unwrap();
        let blob2 = josh_gix_ext::write_blob(&repo.objects, b"other").unwrap();
        let t = open_transaction(&td);

        // "z.rs" first: canonically misplaced, so a bisect for it fails.
        let unsorted = write_raw_tree(&repo, &[("100644", "z.rs", blob), ("100644", "a.rs", blob)]);

        let selector = write_raw_tree(&repo, &[("100644", "z.rs", blob)]);

        let out = subtract(&t, unsorted, selector).unwrap();
        let out_tree = out_entries(&t, out);
        assert!(
            out_entry(&out_tree, "z.rs").is_none(),
            "subtract must find z.rs despite the non-canonical order"
        );
        assert!(out_entry(&out_tree, "a.rs").is_some());

        // Overlaying a new name onto the unsorted tree: existing entries keep their order,
        // the new entry lands after the last entry that canonically precedes it (here: at
        // the end, after a.rs).
        let addition = write_raw_tree(&repo, &[("100644", "m.rs", blob2)]);
        let out = overlay(&t, unsorted, addition).unwrap();
        let expected = write_raw_tree(
            &repo,
            &[
                ("100644", "z.rs", blob),
                ("100644", "a.rs", blob),
                ("100644", "m.rs", blob2),
            ],
        );
        assert_eq!(
            out, expected,
            "existing order preserved, insertion best-effort"
        );
    }

    // Build a tree whose blob contents depend only on the entry NAME (not the full path), so
    // directories with identical children get identical subtree oids -- the precondition for
    // exercising the path-aliasing scenario.
    fn make_named_tree(repo: &gix::Repository, paths: &[String]) -> gix_hash::ObjectId {
        let mut builder = repo.edit_tree(empty_id()).unwrap();
        for path in paths {
            let name = path.rsplit('/').next().unwrap();
            let oid = josh_gix_ext::write_blob(&repo.objects, name.as_bytes()).unwrap();
            builder
                .upsert(path.as_str(), gix::objs::tree::EntryKind::Blob, oid)
                .unwrap();
        }
        builder.write().unwrap().detach()
    }

    // Ground truth for a pattern filter: enumerate every blob path of `input` and keep exactly
    // those the glob crate matches on the FULL path (with the Op::Pattern MatchOptions), rebuilt
    // with a TreeUpdateBuilder (which drops empty dirs). Deliberately NOT remove_pred: the old
    // full-path walk had an order-dependent cache-aliasing bug for identical subtrees at
    // different paths, which the duplicated-subtree case below exercises.
    fn ground_truth_tree(
        repo: &gix::Repository,
        input: gix_hash::ObjectId,
        pattern: &str,
    ) -> gix_hash::ObjectId {
        let glob = glob::Pattern::new(pattern).unwrap();
        let mut kept = vec![];
        josh_gix_ext::walk_tree_preorder(&repo.objects, input, &mut |parent, entry| {
            if entry.mode.is_blob() {
                let name = std::str::from_utf8(entry.filename)?;
                let path = if parent.is_empty() {
                    name.to_owned()
                } else {
                    format!("{parent}/{name}")
                };
                if glob.matches_path_with(Path::new(&path), PATTERN_MATCH_OPTIONS) {
                    kept.push((path, entry.oid.to_owned()));
                }
            }
            Ok(())
        })
        .unwrap();
        let mut builder = repo.edit_tree(empty_id()).unwrap();
        for (path, oid) in &kept {
            builder
                .upsert(path.as_str(), gix::objs::tree::EntryKind::Blob, *oid)
                .unwrap();
        }
        builder.write().unwrap().detach()
    }

    // Property-style equivalence of the component-wise NFA walk against full-path glob matching:
    // a deterministic pseudo-random tree with dotfiles (blobs and subtrees) at multiple depths,
    // >= 5 levels of nesting, varied extensions, and two IDENTICAL subtrees at different paths
    // ("a/x" and "c/x") whose files a path-sensitive pattern must treat differently -- the
    // scenario the old (oid, filter)-keyed cache got wrong depending on walk order.
    #[test]
    fn remove_pattern_matches_full_path_glob() {
        use rand::prelude::*;

        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let mut paths: Vec<String> = [
            // Two identical subtrees at different paths; "a/*/b" and "a/**/b" keep only the "a/"
            // side.
            "a/x/b",
            "a/x/c.rs",
            "c/x/b",
            "c/x/c.rs",
            // Dotfiles at multiple depths: blobs and whole dot-led subtrees.
            ".hidden",
            ".hiddendir/inner.rs",
            "d/.hidden",
            "d/.hiddendir/deep/x.rs",
            "dir_02/sub/.hidden",
            // Deep nesting.
            "dir_01/l1/l2/l3/l4/l5/deep.rs",
            "top.rs",
            "top.txt",
        ]
        .map(String::from)
        .to_vec();

        // Randomized bulk of the tree, deterministic via a fixed seed. Directory names avoid
        // "a"/"c" so the identical-subtree pair above stays identical.
        let mut rng = StdRng::seed_from_u64(42);
        let dirs = ["dir_00", "dir_01", "dir_02", ".dotdir", "sub", "nested"];
        let stems = ["file", "lib", "main", "x", ".hidden", "mod"];
        let exts = ["rs", "txt", "c"];
        for _ in 0..150 {
            let depth = rng.random_range(1..=6);
            let mut p = String::new();
            for _ in 0..depth - 1 {
                p.push_str(dirs[rng.random_range(0..dirs.len())]);
                p.push('/');
            }
            let stem = stems[rng.random_range(0..stems.len())];
            let ext = exts[rng.random_range(0..exts.len())];
            p.push_str(&format!("{stem}_{}.{ext}", rng.random_range(0..8)));
            paths.push(p);
        }
        // Drop paths that collide with another path as a directory prefix (a TreeUpdateBuilder
        // cannot hold "x" as both blob and dir), keeping the first occurrence.
        paths.sort();
        paths.dedup();
        let paths: Vec<String> = paths
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                !paths.iter().enumerate().any(|(j, q)| {
                    *i != j && (q.starts_with(&format!("{p}/")) || p.starts_with(&format!("{q}/")))
                })
            })
            .map(|(_, p)| p.clone())
            .collect();

        let input = make_named_tree(&repo, &paths);

        assert_eq!(
            objects::path_entry(&repo.objects, input, Path::new("a/x"))
                .unwrap()
                .unwrap()
                .oid,
            objects::path_entry(&repo.objects, input, Path::new("c/x"))
                .unwrap()
                .unwrap()
                .oid,
            "a/x and c/x must be identical subtrees for the aliasing case to be exercised"
        );

        let t = open_transaction(&td);
        for pattern in [
            "**/*.rs",
            "*.rs",
            "a/*/b",
            "a/**/b",
            "dir_0?/**",
            "**/.hidden",
            "**",
            "a/x/*.rs",
            "sub/**",
            "a/**/",
            "**/",
            "[a-d]*/**/*.rs",
            "dir_01/l1/**/l4/*/deep.rs",
            // A '/' inside a bracket class is a class member (which can never match under
            // require_literal_separator), not a separator: the token-based split keeps these
            // on the NFA walk.
            "a[/]b",
            "dir_0[!/]/**",
            "a/**/**/x*.rs",
        ] {
            // Isolate the cases from each other (and from other tests in this process).
            cache::clear_global_caches();

            let key = objects::hash_blob(pattern.as_bytes());
            let cp = CompiledPattern::compile(pattern).unwrap();
            assert!(!cp.fallback, "`{pattern}` must not need the fallback");
            let got =
                remove_pattern(&t, input, &cp, key, CompiledPattern::initial_state()).unwrap();
            let want = ground_truth_tree(&repo, input, pattern);
            assert_eq!(
                got, want,
                "`{pattern}` diverged from full-path glob matching"
            );
        }
    }

    // Only patterns with more than 63 components (the u64 state mask limit) take the fallback;
    // the token-based component split handles every other pattern exactly. The fallback itself
    // must produce ground-truth results with per-root cache keys.
    #[test]
    fn compiled_pattern_fallback_cases() {
        let key = gix_hash::ObjectId::from_str("1234567890123456789012345678901234567890").unwrap();

        // A '/' inside a bracket class never splits: it is inside the class token, not a
        // `Char('/')` token, so these stay on the NFA walk.
        assert!(!CompiledPattern::compile("a[/]b").unwrap().fallback);
        assert!(!CompiledPattern::compile("a[!/]b").unwrap().fallback);
        // A ']' in first member position is a literal member, not the closing bracket.
        assert!(!CompiledPattern::compile("a[]]b").unwrap().fallback);
        assert!(!CompiledPattern::compile("a[!]]b").unwrap().fallback);
        // 64 components exceed the u64 state mask limit; 63 still fit.
        let components_64 = format!("{}a", "a/".repeat(63));
        let components_63 = format!("{}a", "a/".repeat(62));
        assert!(CompiledPattern::compile(&components_64).unwrap().fallback);
        assert!(!CompiledPattern::compile(&components_63).unwrap().fallback);
        // Whole-pattern compile errors stay bit-identical.
        assert!(CompiledPattern::compile("a**").is_err());

        // The fallback walk must still match full paths correctly for a fallback pattern.
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let paths: Vec<String> = ["a/f.txt", "b/f.txt"].map(String::from).to_vec();
        let input = make_named_tree(&repo, &paths);
        let t = open_transaction(&td);
        let cp = CompiledPattern::compile(&components_64).unwrap();
        let out = remove_pred(
            &t,
            &mut String::new(),
            input,
            &|path, isblob| isblob && cp.full.matches_with(path, PATTERN_MATCH_OPTIONS),
            key,
        )
        .unwrap();
        assert_eq!(
            out,
            empty_id(),
            "a 64-component pattern matches nothing here"
        );
    }

    // The remove_pred fallback keys its cache by (subtree, pattern, root path), so identical
    // subtrees at different paths must not alias even in the legacy walk.
    #[test]
    fn remove_pred_does_not_alias_identical_subtrees() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let paths: Vec<String> = ["a/f.txt", "b/f.txt"].map(String::from).to_vec();
        let input = make_named_tree(&repo, &paths);
        assert_eq!(
            objects::path_entry(&repo.objects, input, Path::new("a"))
                .unwrap()
                .unwrap()
                .oid,
            objects::path_entry(&repo.objects, input, Path::new("b"))
                .unwrap()
                .unwrap()
                .oid
        );

        let t = open_transaction(&td);
        let pattern = glob::Pattern::new("a/*.txt").unwrap();
        let key = gix_hash::ObjectId::from_str("abcdef1234567890123456789012345678901234").unwrap();
        let out = remove_pred(
            &t,
            &mut String::new(),
            input,
            &|path, isblob| isblob && pattern.matches_with(path, PATTERN_MATCH_OPTIONS),
            key,
        )
        .unwrap();
        let want = ground_truth_tree(&repo, input, "a/*.txt");
        assert_eq!(out, want, "b/f.txt must not be kept via the a/ cache entry");
    }

    // get_path_entry misses on missing components and non-tree intermediates, resolves final
    // entries of any kind (gitlinks included), normalizes redundant path syntax, tolerates
    // non-canonical trees via the linear-lookup fallback, and reads the virtualized empty
    // tree.
    #[test]
    fn get_path_entry_contract() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();
        let blob = josh_gix_ext::write_blob(&repo.objects, b"content").unwrap();
        let t = open_transaction(&td);
        let odb = t.odb();

        let gitlink =
            gix_hash::ObjectId::from_str("0123456789012345678901234567890123456789").unwrap();
        let mut builder = repo.edit_tree(empty_id()).unwrap();
        builder
            .upsert("a/b/deep.txt", gix::objs::tree::EntryKind::Blob, blob)
            .unwrap();
        builder
            .upsert("top.txt", gix::objs::tree::EntryKind::Blob, blob)
            .unwrap();
        builder
            .upsert("a/sub", gix::objs::tree::EntryKind::Commit, gitlink)
            .unwrap();
        let root = builder.write().unwrap().detach();

        let entry = get_path_entry(&t, odb, root, Path::new("a/b/deep.txt"))
            .unwrap()
            .expect("hit at depth 2");
        assert_eq!(entry.oid, blob);
        assert!(!entry.mode.is_tree());

        let entry = get_path_entry(&t, odb, root, Path::new("a/sub"))
            .unwrap()
            .expect("gitlink entry resolves");
        assert!(entry.mode.is_commit());

        for miss in ["a/b/missing.txt", "a/missing/deep.txt", "top.txt/below"] {
            assert!(
                get_path_entry(&t, odb, root, Path::new(miss))
                    .unwrap()
                    .is_none(),
                "{miss} must be a miss"
            );
        }

        // Path normalization: `.` components and redundant separators are ignored, while
        // absolute paths and `..` can never name a tree entry and read as misses.
        for hit in ["a/./b/deep.txt", "a//b/deep.txt", "./top.txt", "a/b/"] {
            assert!(
                get_path_entry(&t, odb, root, Path::new(hit))
                    .unwrap()
                    .is_some(),
                "{hit} must resolve"
            );
        }
        for miss in ["../top.txt", "/top.txt", "."] {
            assert!(
                get_path_entry(&t, odb, root, Path::new(miss))
                    .unwrap()
                    .is_none(),
                "{miss} must be a miss"
            );
        }

        // The empty tree reads through the virtualized disk fallback: no entries, plain miss.
        assert!(
            get_path_entry(&t, odb, empty_id(), Path::new("anything"))
                .unwrap()
                .is_none()
        );
    }

    // Removing a whole subdirectory must report every file under it as removed. This exercises
    // the "input1 is a tree, input2 is gone" branch of diff_paths, which is only reachable via
    // the recursion for entries present in tree1 but absent from tree2.
    #[test]
    fn diff_paths_reports_removed_subtree() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let tree1 = make_tree(&repo, &["dir/file1", "dir/file2", "kept"]);
        let tree2 = make_tree(&repo, &["kept"]);

        let removed = diff_paths(&repo.objects, tree1, tree2, "").unwrap();
        assert_eq!(
            removed,
            vec![("dir/file1".to_string(), -1), ("dir/file2".to_string(), -1)]
        );

        let added = diff_paths(&repo.objects, tree2, tree1, "").unwrap();
        assert_eq!(
            added,
            vec![("dir/file1".to_string(), 1), ("dir/file2".to_string(), 1)]
        );
    }

    // Subtract starts from tree1's entries the way the seeded treebuilder did: untouched entries
    // keep their raw (even fsck-invalid legacy) modes, while matched paths are removed or
    // replaced by the recursive subtraction result.
    #[test]
    fn subtract_preserves_untouched_raw_modes() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let blob = josh_gix_ext::write_blob(&repo.objects, b"legacy").unwrap();
        let sub_tree = make_tree(&repo, &["dir/a.txt", "dir/b.txt"]);
        let dir = objects::path_entry(&repo.objects, sub_tree, Path::new("dir"))
            .unwrap()
            .unwrap()
            .oid;
        let input1 = write_raw_tree(
            &repo,
            &[("40000", "dir", dir), ("100664", "legacy.rs", blob)],
        );
        let input2 = make_tree(&repo, &["dir/a.txt"]);

        let t = open_transaction(&td);
        let out = subtract(&t, input1, input2).unwrap();

        let out_tree = out_entries(&t, out);
        assert_eq!(
            out_entry(&out_tree, "legacy.rs").unwrap().mode.value(),
            0o100664,
            "untouched entries must keep their raw mode, like the seeded treebuilder"
        );
        let out_dir = out_entries(&t, out_entry(&out_tree, "dir").unwrap().oid);
        assert!(
            out_entry(&out_dir, "a.txt").is_none(),
            "matched path removed"
        );
        assert!(
            out_entry(&out_dir, "b.txt").is_some(),
            "unmatched path kept"
        );
    }

    // Overlay resolves blob collisions in favor of input1 and takes over input2-only entries
    // with normalized modes, while input1-only entries keep their raw modes (seeded rebuild).
    #[test]
    fn overlay_keeps_input1_on_collision() {
        let td = tempfile::tempdir().unwrap();
        let repo = gix::init_bare(td.path()).unwrap();

        let ours = josh_gix_ext::write_blob(&repo.objects, b"ours").unwrap();
        let theirs = josh_gix_ext::write_blob(&repo.objects, b"theirs").unwrap();
        let input1 = write_raw_tree(&repo, &[("100664", "shared.rs", ours)]);
        let input2 = write_raw_tree(
            &repo,
            &[
                ("100644", "new.rs", theirs),
                ("100644", "shared.rs", theirs),
            ],
        );

        let t = open_transaction(&td);
        let out = overlay(&t, input1, input2).unwrap();

        let out_tree = out_entries(&t, out);
        let shared = out_entry(&out_tree, "shared.rs").unwrap();
        assert_eq!(shared.oid, ours, "input1 wins blob collisions");
        assert_eq!(
            shared.mode.value(),
            0o100664,
            "collision entries keep input1's raw mode"
        );
        assert_eq!(out_entry(&out_tree, "new.rs").unwrap().oid, theirs);
    }
}
