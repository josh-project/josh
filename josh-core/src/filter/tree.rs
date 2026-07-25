use super::*;
use crate::objects;
use anyhow::anyhow;

pub fn pathstree<'a>(
    root: &str,
    input: git2::Oid,
    transaction: &'a cache::Transaction,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    let odb = repo.odb()?;
    let result = pathstree_inner(root, input, transaction, &odb)?;
    Ok(repo.find_tree(result)?)
}

/// Oid-level body of [`pathstree`]; the odb is hoisted like in [`remove_pred_inner`].
fn pathstree_inner(
    root: &str,
    input: git2::Oid,
    transaction: &cache::Transaction,
    odb: &git2::Odb,
) -> anyhow::Result<git2::Oid> {
    if let Some(cached) = transaction.get_paths((input, root.to_string())) {
        return Ok(cached);
    }

    let repo = transaction.repo();
    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("pathstree: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());

    for entry in &tree.entries {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
        if entry.mode.is_tree() {
            let s = pathstree_inner(
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                objects::git2_oid(entry.oid),
                transaction,
                odb,
            )?;

            if s != empty_id() && objects::tree_entry_name_valid(entry.filename) {
                rebuild.keep(gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: entry.filename.to_owned(),
                    oid: objects::gix_oid(s),
                });
            }
        } else if !entry.mode.is_commit() {
            // Blob-classified entries (including symlinks); gitlinks are skipped.
            let path = normalize_path(&Path::new(root).join(name));
            let path_string = path.to_str().ok_or_else(|| anyhow!("no name"))?;
            let file_contents = if name == "workspace.josh" {
                format!(
                    "#{}\n{}",
                    path_string,
                    blob_text(repo, objects::git2_oid(entry.oid))
                )
            } else {
                path_string.to_string()
            };
            if objects::tree_entry_name_valid(entry.filename) {
                rebuild.keep(gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Blob.into(),
                    filename: entry.filename.to_owned(),
                    oid: objects::gix_oid(repo.blob(file_contents.as_bytes())?),
                });
            }
        }
    }
    let result = objects::write_tree_now(odb, rebuild.out)?;
    transaction.insert_paths((input, root.to_string()), result);
    Ok(result)
}

pub fn regex_replace<'a>(
    input: git2::Oid,
    regex: &regex::Regex,
    replacement: &str,
    transaction: &'a cache::Transaction,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    let odb = repo.odb()?;
    let result = regex_replace_inner(input, regex, replacement, transaction, &odb)?;
    Ok(repo.find_tree(result)?)
}

/// Oid-level body of [`regex_replace`]; the odb is hoisted like in [`remove_pred_inner`].
fn regex_replace_inner(
    input: git2::Oid,
    regex: &regex::Regex,
    replacement: &str,
    transaction: &cache::Transaction,
    odb: &git2::Odb,
) -> anyhow::Result<git2::Oid> {
    let repo = transaction.repo();
    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("regex_replace: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());

    for entry in &tree.entries {
        // Non-UTF-8 entry names stay an error even though the name is otherwise unused here.
        std::str::from_utf8(entry.filename).map_err(|_| anyhow!("no name"))?;
        let raw_mode = entry.mode.value();
        if entry.mode.is_tree() {
            let s = regex_replace_inner(
                objects::git2_oid(entry.oid),
                regex,
                replacement,
                transaction,
                odb,
            )?;

            if s != tree::empty_id() && objects::tree_entry_name_valid(entry.filename) {
                rebuild.keep(gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Tree.into(),
                    filename: entry.filename.to_owned(),
                    oid: objects::gix_oid(s),
                });
            }
        } else if !entry.mode.is_commit() {
            let file_contents = blob_text(repo, objects::git2_oid(entry.oid));
            let replaced = regex.replacen(&file_contents, 0, replacement);

            if objects::tree_entry_name_valid(entry.filename) {
                rebuild.keep(gix_object::tree::Entry {
                    mode: entry_mode(objects::normalize_filemode(raw_mode)),
                    filename: entry.filename.to_owned(),
                    oid: objects::gix_oid(repo.blob(replaced.as_bytes())?),
                });
            }
        }
    }
    objects::write_tree_now(odb, rebuild.out)
}

/// The text content of the blob `oid`, or the empty string when the blob is missing, binary, or
/// not valid UTF-8 -- the same tolerant semantics as [`get_blob`], minus the path lookup.
fn blob_text(repo: &git2::Repository, oid: git2::Oid) -> String {
    let blob = ok_or!(repo.find_blob(oid), {
        return "".to_owned();
    });
    if blob.is_binary() {
        return "".to_owned();
    }
    let content = ok_or!(std::str::from_utf8(blob.content()), {
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

/// Accumulator for rebuilding a tree entry-by-entry, encapsulating the libgit2-treebuilder
/// parity rules: canonical-order tracking, last-wins dedup of duplicate names, and the
/// unchanged fast path guarded by `changed`. Callers report the fate of each input entry
/// (`keep` for survivors, `mark_changed` for anything dropped, rewritten, renamed-away or
/// mode-normalized) and `finish` writes the rebuilt tree or returns the input oid.
///
/// TODO(byte-preservation): the last-wins dedup in `keep`, the order-violation handling
/// (forcing `changed` so non-canonical trees skip the unchanged fast path), and the
/// unconditional sort in `write_tree_now` together replicate libgit2's treebuilder
/// normalization of fsck-invalid input trees. That normalization is a bug of the same kind as
/// git2's gpgsig header line-ending normalization: it re-encodes bytes the operation was
/// never asked to change, so non-canonical trees do not round-trip through a filter even when
/// it keeps them entirely. The intended semantics is byte-preservation: pass fully-kept
/// subtrees through verbatim (an order violation alone must not set `changed`), and when a
/// rebuild is forced by dropping or replacing entries, keep the surviving entries in their
/// original relative order, duplicates included -- duplicates share a name, so a path
/// predicate keeps or drops them together. This needs an order-preserving alternative to
/// `write_tree_now` and tolerant (non-bisecting) name lookups in subtract/intersect/overlay,
/// and it changes filtered oids for repos containing such trees, so it must land as its own
/// change with tests -- not inside the gix port, which is validated on byte-identity against
/// the libgit2 baseline and keeps this wart until then.
struct TreeRebuild {
    out: Vec<gix_object::tree::Entry>,
    /// Whether the rebuilt entry set could serialize to anything other than the input
    /// byte-for-byte. While it stays false, `finish` returns the input oid without writing a
    /// tree; any dropped, rewritten, renamed-away or mode-normalized entry sets it, as does a
    /// non-canonical input encoding (which re-serialization would not reproduce).
    changed: bool,
    /// Set once a canonical-order violation or duplicate name has been observed among the
    /// kept entries; gates the dedup scan in `keep` so canonical input trees -- the hot
    /// path -- never scan.
    order_violation: bool,
    /// Previous kept entry's name and kind, for the canonical-order check.
    prev: Option<(gix_object::bstr::BString, bool)>,
}

impl TreeRebuild {
    fn new(capacity: usize) -> Self {
        Self {
            out: Vec::with_capacity(capacity),
            changed: false,
            order_violation: false,
            prev: None,
        }
    }

    /// Record that the rebuilt tree differs from the input: an entry was dropped, rewritten,
    /// renamed away or had its mode normalized.
    fn mark_changed(&mut self) {
        self.changed = true;
    }

    /// Append a surviving entry, replacing an earlier kept entry of the same name like
    /// libgit2's name-keyed treebuilder did (last insert wins).
    ///
    /// Also verifies the kept entries arrive in canonical git order with no duplicate names:
    /// a violation forces `changed` (non-canonical input trees must not take the unchanged
    /// fast path) and arms the dedup scan, since duplicate names imply one (equal names
    /// adjacent, or a sort-order break in between). Tracking only the *kept* entries is
    /// sufficient: violations adjacent to dropped entries already force `changed` via the
    /// drop, and a duplicate name among the kept entries always shows up as a violation in
    /// the kept stream at or before its second occurrence -- a strictly increasing sequence
    /// cannot revisit a name.
    fn keep(&mut self, entry: gix_object::tree::Entry) {
        let is_tree = entry.mode.is_tree();
        if let Some((prev_name, prev_is_tree)) = &self.prev {
            if prev_name[..] == entry.filename[..]
                || git_tree_entry_cmp(prev_name, *prev_is_tree, &entry.filename, is_tree)
                    != std::cmp::Ordering::Less
            {
                self.order_violation = true;
                self.changed = true;
            }
        }
        self.prev = Some((entry.filename.clone(), is_tree));
        if self.order_violation {
            if let Some(pos) = self.out.iter().rposition(|e| e.filename == entry.filename) {
                self.out[pos] = entry;
                return;
            }
        }
        self.out.push(entry);
    }

    /// Write the rebuilt tree, or return `input` unchanged when nothing was dropped or
    /// rewritten: an untouched entry set reproduces `input` bit-identically, since git trees
    /// are content-addressed.
    fn finish(self, odb: &git2::Odb, input: git2::Oid) -> anyhow::Result<git2::Oid> {
        if self.changed {
            objects::write_tree_now(odb, self.out)
        } else {
            #[cfg(debug_assertions)]
            debug_assert_eq!(objects::write_tree_now(odb, self.out)?, input);
            Ok(input)
        }
    }
}

/// Clone a parsed tree's entries for use as the starting set of a rebuild, replicating what
/// seeding a libgit2 treebuilder from a tree did: entry names and raw modes are taken over
/// unvalidated and unnormalized, but duplicate names collapse to the last occurrence (the
/// treebuilder was a name-keyed map). [`TreeRebuild::keep`] owns that dedup and the
/// order-violation detection arming it; `keep` is usable standalone here because seeded
/// rebuilds always write via `write_tree_now` and never consult the `changed` flag it also
/// maintains.
fn seed_entries(tree: &gix_object::TreeRef) -> Vec<gix_object::tree::Entry> {
    let mut rebuild = TreeRebuild::new(tree.entries.len());
    for entry in &tree.entries {
        rebuild.keep((*entry).into());
    }
    rebuild.out
}

/// Look up an entry by name irrespective of its kind, like libgit2's `git_tree_entry_byname`.
/// Canonical order sorts a blob before a same-named tree, so the blob probe comes first.
fn lookup_entry<'a>(
    tree: &gix_object::TreeRef<'a>,
    name: &gix_object::bstr::BStr,
) -> Option<gix_object::tree::EntryRef<'a>> {
    tree.bisect_entry(name, false)
        .or_else(|| tree.bisect_entry(name, true))
}

/// The [`gix_object::tree::EntryMode`] for a libgit2-normalized mode value.
fn entry_mode(norm: u16) -> gix_object::tree::EntryMode {
    gix_object::tree::EntryMode::try_from(norm as u32).expect("normalized modes are valid")
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
    input: git2::Oid,
    pred: &dyn Fn(&str, bool) -> bool,
    key: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let odb = transaction.repo().odb()?;
    remove_pred_inner(transaction, &odb, path, input, pred, key)
}

/// Recursive body of [`remove_pred`], with the odb handle created once at the entry point --
/// creating one per recursion level costs an FFI round-trip per tree node.
fn remove_pred_inner(
    transaction: &cache::Transaction,
    odb: &git2::Odb,
    path: &mut String,
    input: git2::Oid,
    pred: &dyn Fn(&str, bool) -> bool,
    key: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let root_key = git2::Oid::hash_object(
        git2::ObjectType::Blob,
        format!("glob-fallback:{:?}:{}", key, path).as_bytes(),
    )?;
    if let Some(cached) = transaction.get_glob((input, root_key, 0)) {
        return Ok(cached);
    }

    let bytes = transaction
        .read_tree_bytes(odb, input)?
        .ok_or_else(|| anyhow!("remove_pred: {} is not a tree", input))?;
    let tree = gix_object::TreeRef::from_bytes(&bytes)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());
    let empty = empty_id();

    for entry in &tree.entries {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("INVALID_FILENAME"))?;
        let base = path.len();
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(name);

        let raw_mode = entry.mode.value();
        if entry.mode.is_tree() {
            let s = remove_pred_inner(
                transaction,
                odb,
                path,
                objects::git2_oid(entry.oid),
                pred,
                key,
            )?;
            // `entry.mode` compares by the serialized form, so the fsck-invalid "040000"
            // spelling also counts as changed (libgit2 could not distinguish it).
            if s != objects::git2_oid(entry.oid)
                || s == empty
                || entry.mode != gix_object::tree::EntryKind::Tree.into()
            {
                rebuild.mark_changed();
            }
            if s != empty {
                // Entries with invalid names (like ".git") are dropped, exactly as a failed
                // `git_treebuilder_insert` silently did, and also count as changed.
                if objects::tree_entry_name_valid(entry.filename) {
                    rebuild.keep(gix_object::tree::Entry {
                        mode: gix_object::tree::EntryKind::Tree.into(),
                        filename: entry.filename.to_owned(),
                        oid: objects::gix_oid(s),
                    });
                } else {
                    rebuild.mark_changed();
                }
            }
        } else if entry.mode.is_commit() {
            // Gitlinks are dropped, so the rebuilt tree differs from the input and must not
            // take the unchanged fast path.
            rebuild.mark_changed();
        } else if pred(path, true) {
            // The normalized mode is what libgit2's treebuilder would have stored: if it
            // differs from the raw on-disk mode (e.g. legacy 100664 blobs), the rebuilt tree
            // differs from the input.
            let norm = objects::normalize_filemode(raw_mode);
            if raw_mode != norm {
                rebuild.mark_changed();
            }
            if objects::tree_entry_name_valid(entry.filename) {
                rebuild.keep(gix_object::tree::Entry {
                    mode: gix_object::tree::EntryMode::try_from(norm as u32)
                        .expect("normalized modes are valid"),
                    filename: entry.filename.to_owned(),
                    oid: entry.oid.to_owned(),
                });
            } else {
                rebuild.mark_changed();
            }
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
    input: git2::Oid,
    cp: &CompiledPattern,
    key: git2::Oid,
    state: u64,
) -> anyhow::Result<git2::Oid> {
    let odb = transaction.repo().odb()?;
    remove_pattern_inner(transaction, &odb, input, cp, key, state)
}

/// Recursive body of [`remove_pattern`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn remove_pattern_inner(
    transaction: &cache::Transaction,
    odb: &git2::Odb,
    input: git2::Oid,
    cp: &CompiledPattern,
    key: git2::Oid,
    state: u64,
) -> anyhow::Result<git2::Oid> {
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
    let tree = gix_object::TreeRef::from_bytes(&bytes)?;
    let mut rebuild = TreeRebuild::new(tree.entries.len());
    let empty = empty_id();

    for entry in &tree.entries {
        let name = std::str::from_utf8(entry.filename).map_err(|_| anyhow!("INVALID_FILENAME"))?;

        let raw_mode = entry.mode.value();
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
                remove_pattern_inner(
                    transaction,
                    odb,
                    objects::git2_oid(entry.oid),
                    cp,
                    key,
                    next,
                )?
            };
            if s != objects::git2_oid(entry.oid)
                || s == empty
                || entry.mode != gix_object::tree::EntryKind::Tree.into()
            {
                rebuild.mark_changed();
            }
            if s != empty {
                if objects::tree_entry_name_valid(entry.filename) {
                    rebuild.keep(gix_object::tree::Entry {
                        mode: gix_object::tree::EntryKind::Tree.into(),
                        filename: entry.filename.to_owned(),
                        oid: objects::gix_oid(s),
                    });
                } else {
                    rebuild.mark_changed();
                }
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
                // See remove_pred for why legacy filemodes and rejected names count as
                // changed.
                let norm = objects::normalize_filemode(raw_mode);
                if raw_mode != norm {
                    rebuild.mark_changed();
                }
                if objects::tree_entry_name_valid(entry.filename) {
                    rebuild.keep(gix_object::tree::Entry {
                        mode: gix_object::tree::EntryMode::try_from(norm as u32)
                            .expect("normalized modes are valid"),
                        filename: entry.filename.to_owned(),
                        oid: entry.oid.to_owned(),
                    });
                } else {
                    rebuild.mark_changed();
                }
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
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let odb = transaction.repo().odb()?;
    subtract_inner(transaction, &odb, input1, input2)
}

/// Recursive body of [`subtract`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn subtract_inner(
    transaction: &cache::Transaction,
    odb: &git2::Odb,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
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
        let tree1 = gix_object::TreeRef::from_bytes(&bytes1)?;
        let tree2 = gix_object::TreeRef::from_bytes(&bytes2)?;
        // Start from `tree1` and drop or replace each path that also appears in `tree2`.
        // Modifications are collected by name first and applied in one pass, mirroring the
        // name-keyed libgit2 treebuilder: `None` removes the entry, `Some` replaces its oid with
        // the subtraction result (a tree, hence the normalized tree mode). Names that libgit2's
        // insert would have rejected silently keep their original entry.
        let mut mods: std::collections::HashMap<&[u8], Option<git2::Oid>> =
            std::collections::HashMap::new();
        for entry in &tree2.entries {
            if let Some(e1) = lookup_entry(&tree1, entry.filename) {
                let sub = subtract_inner(
                    transaction,
                    odb,
                    objects::git2_oid(e1.oid),
                    objects::git2_oid(entry.oid),
                )?;
                if sub == empty_id() || sub == git2::Oid::zero() {
                    mods.insert(&**entry.filename, None);
                } else if objects::tree_entry_name_valid(entry.filename) {
                    mods.insert(&**entry.filename, Some(sub));
                }
            }
        }

        let mut out = seed_entries(&tree1);
        out.retain(|e| mods.get(e.filename.as_slice()) != Some(&None));
        for entry in &mut out {
            if let Some(Some(sub)) = mods.get(entry.filename.as_slice()) {
                entry.mode = gix_object::tree::EntryKind::Tree.into();
                entry.oid = objects::gix_oid(*sub);
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
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let odb = transaction.repo().odb()?;
    intersect_inner(transaction, &odb, input1, input2)
}

/// Recursive body of [`intersect`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn intersect_inner(
    transaction: &cache::Transaction,
    odb: &git2::Odb,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
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
        let tree1 = gix_object::TreeRef::from_bytes(&bytes1)?;
        let tree2 = gix_object::TreeRef::from_bytes(&bytes2)?;
        // Iterate the selector (`input2`), keeping each of its paths that also exists in `tree1`
        // with `tree1`'s content and normalized mode; cost tracks the size of the selected set.
        // Entries with invalid names are dropped silently (libgit2 insert-name parity).
        let mut rebuild = TreeRebuild::new(0);
        for entry in &tree2.entries {
            if let Some(e1) = lookup_entry(&tree1, entry.filename) {
                let child = intersect(
                    transaction,
                    objects::git2_oid(e1.oid),
                    objects::git2_oid(entry.oid),
                )?;
                if child != empty_id()
                    && child != git2::Oid::zero()
                    && objects::tree_entry_name_valid(entry.filename)
                {
                    rebuild.keep(gix_object::tree::Entry {
                        mode: entry_mode(objects::normalize_filemode(e1.mode.value())),
                        filename: entry.filename.to_owned(),
                        oid: objects::gix_oid(child),
                    });
                }
            }
        }
        objects::write_tree_now(odb, rebuild.out)?
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
    std::os::unix::ffi::OsStrExt::as_bytes(c)
}

/// Read `oid` as a raw tree object, or `None` if it is missing or not a tree. Uncached: the
/// insert path is used from repository-only contexts (josh-link, josh-changes) that have no
/// transaction to hang the tree cache off.
fn tree_object<'a>(odb: &'a git2::Odb, oid: git2::Oid) -> Option<git2::OdbObject<'a>> {
    match odb.read(oid) {
        Ok(obj) if obj.kind() == git2::ObjectType::Tree => Some(obj),
        _ => None,
    }
}

/// Rebuild the tree `tree_oid` with the single-component `child` replaced by (`oid`, `mode`), or
/// removed when `oid` is the zero or empty-tree oid. Like a seeded libgit2 treebuilder: other
/// entries keep their raw modes, the inserted mode is normalized, and an invalid child name
/// silently leaves the tree unchanged.
fn replace_child_inner(
    odb: &git2::Odb,
    child: &[u8],
    oid: git2::Oid,
    mode: i32,
    tree_oid: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let mut out = match tree_object(odb, tree_oid) {
        Some(obj) => seed_entries(&gix_object::TreeRef::from_bytes(obj.data())?),
        None => Vec::new(),
    };
    let remove = oid == git2::Oid::zero() || oid == empty_id();
    let existing = out.iter().position(|e| &*e.filename == child);
    if remove {
        if let Some(pos) = existing {
            out.remove(pos);
        }
    } else if objects::tree_entry_name_valid(child) {
        let entry = gix_object::tree::Entry {
            mode: entry_mode(objects::normalize_filemode(mode as u16)),
            filename: child.into(),
            oid: objects::gix_oid(oid),
        };
        if let Some(pos) = existing {
            out[pos] = entry;
        } else {
            out.push(entry);
        }
    }
    objects::write_tree_now(odb, out)
}

/// Oid-level body of [`insert`]: replace whatever is at `path` inside the tree `full_tree` with
/// (`oid`, `mode`), creating intermediate trees as needed and treating blobs on the way as
/// overwritable.
fn insert_inner(
    odb: &git2::Odb,
    full_tree: git2::Oid,
    path: &Path,
    oid: git2::Oid,
    mode: i32,
) -> anyhow::Result<git2::Oid> {
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
        st = match tree_object(odb, st) {
            Some(obj) => {
                let tree = gix_object::TreeRef::from_bytes(obj.data())?;
                match lookup_entry(&tree, cb.into()) {
                    Some(e) => objects::git2_oid(e.oid),
                    None => empty_id(),
                }
            }
            None => empty_id(),
        };
    }

    let subtree = replace_child_inner(odb, component_bytes(name), oid, mode, st)?;

    insert_inner(odb, full_tree, parent, subtree, 0o0040000)
}

pub fn insert<'a>(
    repo: &'a git2::Repository,
    full_tree: &git2::Tree,
    path: &Path,
    oid: git2::Oid,
    mode: i32,
) -> anyhow::Result<git2::Tree<'a>> {
    let odb = repo.odb()?;
    let result = insert_inner(&odb, full_tree.id(), path, oid, mode)?;
    Ok(repo.find_tree(result)?)
}

pub fn diff_paths(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
    root: &str,
) -> anyhow::Result<Vec<(String, i32)>> {
    if input1 == input2 {
        return Ok(vec![]);
    }

    if let (Ok(_), Ok(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), 0)]);
    }

    if let (Ok(_), Err(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), -1)]);
    }

    if let (Err(_), Ok(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), 1)]);
    }

    let mut r = vec![];

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| anyhow!("no name"))?) {
                r.append(&mut diff_paths(
                    repo,
                    e.id(),
                    entry.id(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            } else {
                r.append(&mut diff_paths(
                    repo,
                    git2::Oid::zero(),
                    entry.id(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        for entry in tree1.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            if tree2
                .get_name(entry.name().ok_or_else(|| anyhow!("no name"))?)
                .is_none()
            {
                r.append(&mut diff_paths(
                    repo,
                    entry.id(),
                    git2::Oid::zero(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        return Ok(r);
    }

    if let Ok(tree2) = repo.find_tree(input2) {
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            r.append(&mut diff_paths(
                repo,
                git2::Oid::zero(),
                entry.id(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    if let Ok(tree1) = repo.find_tree(input1) {
        for entry in tree1.iter() {
            let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
            r.append(&mut diff_paths(
                repo,
                entry.id(),
                git2::Oid::zero(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    Ok(r)
}

pub fn overlay(
    transaction: &cache::Transaction,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    let odb = transaction.repo().odb()?;
    overlay_inner(transaction, &odb, input1, input2)
}

/// Recursive body of [`overlay`]; see [`remove_pred_inner`] for why the odb is hoisted.
fn overlay_inner(
    transaction: &cache::Transaction,
    odb: &git2::Odb,
    input1: git2::Oid,
    input2: git2::Oid,
) -> anyhow::Result<git2::Oid> {
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
        let tree1 = gix_object::TreeRef::from_bytes(&bytes1)?;
        let tree2 = gix_object::TreeRef::from_bytes(&bytes2)?;
        // Start from `tree1` and insert every entry of `tree2`: recursively overlaid where the
        // name exists in `tree1` (with `tree1` winning on blob collisions), taken over as-is
        // otherwise. Every inserted entry gets the normalized mode, and -- unlike the other tree
        // ops -- an invalid entry name is an error.
        let mut mods: std::collections::HashMap<&[u8], (git2::Oid, gix_object::tree::EntryMode)> =
            std::collections::HashMap::new();
        // Name-keyed, so duplicate names in `tree2` collapse to the last occurrence; the
        // canonical sort in write_tree_now restores order.
        let mut new_entries: std::collections::HashMap<&[u8], gix_object::tree::Entry> =
            std::collections::HashMap::new();
        for entry in &tree2.entries {
            if !objects::tree_entry_name_valid(entry.filename) {
                return Err(anyhow!(
                    "overlay: invalid entry name {:?}",
                    entry.filename.to_owned()
                ));
            }
            if let Some(e1) = lookup_entry(&tree1, entry.filename) {
                let id = overlay_inner(
                    transaction,
                    odb,
                    objects::git2_oid(e1.oid),
                    objects::git2_oid(entry.oid),
                )?;
                mods.insert(
                    &**entry.filename,
                    (id, entry_mode(objects::normalize_filemode(e1.mode.value()))),
                );
            } else {
                new_entries.insert(
                    &**entry.filename,
                    gix_object::tree::Entry {
                        mode: entry_mode(objects::normalize_filemode(entry.mode.value())),
                        filename: entry.filename.to_owned(),
                        oid: entry.oid.to_owned(),
                    },
                );
            }
        }

        let mut out = seed_entries(&tree1);
        for entry in &mut out {
            if let Some((id, mode)) = mods.get(entry.filename.as_slice()) {
                entry.oid = objects::gix_oid(*id);
                entry.mode = *mode;
            }
        }
        out.extend(new_entries.into_values());

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

pub fn invert_paths<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_invert((tree.id(), root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            let mpath = normalize_path(&Path::new(root).join(name))
                .to_string_lossy()
                .to_string();
            let b = get_blob(repo, &tree, Path::new(name));
            let opath = pathline(&b)?;

            result = insert(
                repo,
                &result,
                Path::new(&opath),
                repo.blob(mpath.as_bytes())?,
                0o0100644,
            )
            .unwrap();
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = invert_paths(
                transaction,
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                repo.find_tree(entry.id())?,
            )?;
            result = repo.find_tree(overlay(transaction, result.id(), s.id())?)?;
        }
    }

    transaction.insert_invert((tree.id(), root.to_string()), result.id());

    Ok(result)
}

pub fn original_path(
    transaction: &cache::Transaction,
    filter: Filter,
    tree: git2::Tree,
    path: &Path,
) -> anyhow::Result<String> {
    let paths_tree = apply(
        transaction,
        to_filter(Op::Paths).chain(filter),
        Rewrite::from_tree(tree),
    )?;
    let b = get_blob(transaction.repo(), paths_tree.tree(), path);
    pathline(&b)
}

pub fn repopulated_tree(
    transaction: &cache::Transaction,
    filter: Filter,
    full_tree: git2::Tree,
    partial_tree: git2::Tree,
) -> anyhow::Result<git2::Oid> {
    let paths_tree = apply(
        transaction,
        to_filter(Op::Paths).chain(filter),
        Rewrite::from_tree(full_tree),
    )?;

    let ipaths = invert_paths(transaction, "", paths_tree.into_tree())?;
    populate(transaction, ipaths.id(), partial_tree.id())
}

pub fn populate(
    transaction: &cache::Transaction,
    paths: git2::Oid,
    content: git2::Oid,
) -> anyhow::Result<git2::Oid> {
    if let Some(cached) = transaction.get_populate((paths, content)) {
        return Ok(cached);
    }

    let repo = transaction.repo();

    let mut result_tree = empty_id();
    if let (Ok(paths), Ok(content)) = (repo.find_blob(paths), repo.find_blob(content)) {
        let ipath = pathline(std::str::from_utf8(paths.content())?)?;
        result_tree = insert(
            repo,
            &repo.find_tree(result_tree)?,
            Path::new(&ipath),
            content.id(),
            0o0100644,
        )?
        .id();
    } else if let (Ok(paths), Ok(content)) = (repo.find_tree(paths), repo.find_tree(content)) {
        for entry in content.iter() {
            if let Some(e) = paths.get_name(entry.name().ok_or_else(|| anyhow!("no name"))?) {
                result_tree = overlay(
                    transaction,
                    result_tree,
                    populate(transaction, e.id(), entry.id())?,
                )?;
            }
        }
    }

    transaction.insert_populate((paths, content), result_tree);

    Ok(result_tree)
}

pub fn compose_fast(
    transaction: &cache::Transaction,
    trees: Vec<git2::Oid>,
) -> anyhow::Result<git2::Tree<'_>> {
    let repo = transaction.repo();
    let mut result = empty_id();
    for tree in trees {
        result = overlay(transaction, tree, result)?;
    }

    Ok(repo.find_tree(result)?)
}

pub fn compose<'a>(
    transaction: &'a cache::Transaction,
    trees: Vec<(&Filter, git2::Tree<'a>)>,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    let mut result = empty(repo);
    let mut taken = empty(repo);
    for (f, applied) in trees {
        let tid = taken.id();
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
            apply(transaction, f, Rewrite::from_tree(taken.clone()))?
                .tree()
                .id()
        };
        transaction.insert_apply(f, tid, taken_applied);

        let subtracted = repo.find_tree(subtract(transaction, applied.id(), taken_applied)?)?;

        let aid = applied.id();
        let unapplied = if let Some(cached) = transaction.get_unapply(f, aid) {
            cached
        } else {
            apply(transaction, invert(f)?, Rewrite::from_tree(applied))?
                .tree()
                .id()
        };
        transaction.insert_unapply(f, aid, unapplied);
        taken = repo.find_tree(overlay(transaction, taken.id(), unapplied)?)?;
        result = repo.find_tree(overlay(transaction, subtracted.id(), result.id())?)?;
    }

    Ok(result)
}

pub fn get_blob(repo: &git2::Repository, tree: &git2::Tree, path: &Path) -> String {
    let entry_oid = ok_or!(tree.get_path(path).map(|x| x.id()), {
        return "".to_owned();
    });

    let blob = ok_or!(repo.find_blob(entry_oid), {
        return "".to_owned();
    });

    if blob.is_binary() {
        return "".to_owned();
    }

    let content = ok_or!(std::str::from_utf8(blob.content()), {
        return "".to_owned();
    });

    content.to_owned()
}

pub fn empty_id() -> git2::Oid {
    git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap()
}

pub fn empty(repo: &git2::Repository) -> git2::Tree<'_> {
    repo.find_tree(empty_id()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(repo: &git2::Repository, paths: &[&str]) -> git2::Oid {
        let mut b = git2::build::TreeUpdateBuilder::new();
        for p in paths {
            let oid = repo.blob(p.as_bytes()).unwrap();
            b.upsert(*p, oid, git2::FileMode::Blob);
        }
        let base = repo.treebuilder(None).unwrap().write().unwrap();
        b.create_updated(repo, &repo.find_tree(base).unwrap())
            .unwrap()
    }

    fn open_transaction(td: &tempfile::TempDir) -> cache::Transaction {
        cache::sled_load(td.path()).unwrap();
        let ctx = cache::TransactionContext::new(td.path(), cache::CacheStack::default().into());
        ctx.open().unwrap()
    }

    // A gitlink (submodule) entry must be dropped from the rebuilt tree -- and must therefore
    // defeat the "unchanged input" fast path -- while a symlink blob accepted by the predicate
    // keeps its 0o120000 filemode.
    #[test]
    fn remove_pred_drops_gitlink_and_preserves_symlink_mode() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let blob = repo.blob(b"content").unwrap();
        let link = repo.blob(b"target").unwrap();
        // Gitlinks reference commits in other repositories; libgit2 does not require the oid
        // to exist locally.
        let sub = git2::Oid::from_str("0123456789012345678901234567890123456789").unwrap();

        let mut b = repo.treebuilder(None).unwrap();
        b.insert("keep.rs", blob, 0o100644).unwrap();
        b.insert("link.rs", link, 0o120000).unwrap();
        b.insert("sub", sub, 0o160000).unwrap();
        let input = b.write().unwrap();

        let t = open_transaction(&td);
        let key = git2::Oid::from_str("1111111111111111111111111111111111111111").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();

        assert_ne!(out, input, "dropping the gitlink must produce a new tree");
        let out_tree = t.repo().find_tree(out).unwrap();
        assert!(
            out_tree.get_name("sub").is_none(),
            "gitlink must be dropped"
        );
        assert!(out_tree.get_name("keep.rs").is_some());
        let link_entry = out_tree.get_name("link.rs").expect("symlink kept");
        assert_eq!(link_entry.filemode(), 0o120000);
        assert_eq!(link_entry.id(), link);
    }

    // The predicate must see full slash-separated paths at every depth (truncate discipline of
    // the shared path buffer), and a keep-everything predicate must return the input oid via the
    // unchanged fast path.
    #[test]
    fn remove_pred_passes_full_paths_and_reuses_unchanged_input() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let paths = ["a/b/drop.txt", "a/b/keep.rs", "a/keep.rs", "top.rs"];
        let input = make_tree(&repo, &paths);

        let t = open_transaction(&td);
        let key = git2::Oid::from_str("2222222222222222222222222222222222222222").unwrap();

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

        let out_tree = t.repo().find_tree(out).unwrap();
        for kept in ["a/b/keep.rs", "a/keep.rs", "top.rs"] {
            assert!(out_tree.get_path(Path::new(kept)).is_ok(), "{kept} kept");
        }
        assert!(out_tree.get_path(Path::new("a/b/drop.txt")).is_err());

        let key2 = git2::Oid::from_str("3333333333333333333333333333333333333333").unwrap();
        let out2 = remove_pred(&t, &mut String::new(), input, &|_, _| true, key2).unwrap();
        assert_eq!(out2, input, "keep-everything must return the input oid");
    }

    // Write a raw (unvalidated) tree object straight into the odb. This can express fsck-invalid
    // trees -- legacy filemodes, unsorted or duplicate entries, forbidden names -- that git can
    // still transport with default settings and that therefore reach remove_pred in production.
    fn write_raw_tree(repo: &git2::Repository, entries: &[(&str, &str, git2::Oid)]) -> git2::Oid {
        let mut data = Vec::new();
        for (mode, name, oid) in entries {
            data.extend_from_slice(mode.as_bytes());
            data.push(b' ');
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(oid.as_bytes());
        }
        repo.odb()
            .unwrap()
            .write(git2::ObjectType::Tree, &data)
            .unwrap()
    }

    // A legacy blob mode like 100664 is normalized by the treebuilder, so a keep-everything
    // predicate must NOT return the raw input oid: it must return the normalized rewrite.
    #[test]
    fn remove_pred_normalizes_legacy_filemode() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let blob = repo.blob(b"content").unwrap();
        let input = write_raw_tree(&repo, &[("100664", "file.rs", blob)]);
        assert_ne!(
            repo.find_tree(input)
                .unwrap()
                .get_name("file.rs")
                .unwrap()
                .filemode_raw(),
            0o100644,
            "input must carry the raw legacy mode"
        );

        let mut b = repo.treebuilder(None).unwrap();
        b.insert("file.rs", blob, 0o100644).unwrap();
        let expected = b.write().unwrap();
        assert_ne!(expected, input);

        let t = open_transaction(&td);
        let key = git2::Oid::from_str("4444444444444444444444444444444444444444").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(
            out, expected,
            "legacy mode must be normalized, not passed through"
        );
    }

    // Entries the treebuilder rejects (".git") must be silently dropped, and non-canonical
    // entry order must be normalized, instead of returning the fsck-invalid input via the
    // fast path.
    #[test]
    fn remove_pred_normalizes_invalid_names_and_entry_order() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();
        let blob = repo.blob(b"content").unwrap();
        let t = open_transaction(&td);

        // ".git" is rejected by the treebuilder and must be dropped, not passed through.
        let input = write_raw_tree(
            &repo,
            &[("100644", ".git", blob), ("100644", "keep.rs", blob)],
        );
        let key = git2::Oid::from_str("5555555555555555555555555555555555555555").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_ne!(out, input);
        let out_tree = t.repo().find_tree(out).unwrap();
        assert!(out_tree.get_name(".git").is_none(), ".git must be dropped");
        assert!(out_tree.get_name("keep.rs").is_some());

        // Unsorted input must be rewritten in canonical order.
        let input = write_raw_tree(&repo, &[("100644", "b.rs", blob), ("100644", "a.rs", blob)]);
        let mut b = repo.treebuilder(None).unwrap();
        b.insert("a.rs", blob, 0o100644).unwrap();
        b.insert("b.rs", blob, 0o100644).unwrap();
        let expected = b.write().unwrap();
        assert_ne!(expected, input);
        let key = git2::Oid::from_str("6666666666666666666666666666666666666666").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(
            out, expected,
            "unsorted input must be rewritten in canonical order"
        );

        // Duplicate names: last one wins in the treebuilder.
        let blob2 = repo.blob(b"other").unwrap();
        let input = write_raw_tree(
            &repo,
            &[("100644", "a.rs", blob), ("100644", "a.rs", blob2)],
        );
        let mut b = repo.treebuilder(None).unwrap();
        b.insert("a.rs", blob2, 0o100644).unwrap();
        let expected = b.write().unwrap();
        let key = git2::Oid::from_str("7777777777777777777777777777777777777777").unwrap();
        let out = remove_pred(&t, &mut String::new(), input, &|_, isblob| isblob, key).unwrap();
        assert_eq!(out, expected, "duplicate entries must be deduplicated");
    }

    // Build a tree whose blob contents depend only on the entry NAME (not the full path), so
    // directories with identical children get identical subtree oids -- the precondition for
    // exercising the path-aliasing scenario.
    fn make_named_tree(repo: &git2::Repository, paths: &[String]) -> git2::Oid {
        let mut b = git2::build::TreeUpdateBuilder::new();
        for p in paths {
            let name = p.rsplit('/').next().unwrap();
            let oid = repo.blob(name.as_bytes()).unwrap();
            b.upsert(p.as_str(), oid, git2::FileMode::Blob);
        }
        let base = repo.treebuilder(None).unwrap().write().unwrap();
        b.create_updated(repo, &repo.find_tree(base).unwrap())
            .unwrap()
    }

    // Ground truth for a pattern filter: enumerate every blob path of `input` and keep exactly
    // those the glob crate matches on the FULL path (with the Op::Pattern MatchOptions), rebuilt
    // with a TreeUpdateBuilder (which drops empty dirs). Deliberately NOT remove_pred: the old
    // full-path walk had an order-dependent cache-aliasing bug for identical subtrees at
    // different paths, which the duplicated-subtree case below exercises.
    fn ground_truth_tree(repo: &git2::Repository, input: git2::Oid, pattern: &str) -> git2::Oid {
        let glob = glob::Pattern::new(pattern).unwrap();
        let tree = repo.find_tree(input).unwrap();
        let mut kept = vec![];
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                let path = format!("{}{}", root, entry.name().unwrap());
                if glob.matches_path_with(Path::new(&path), PATTERN_MATCH_OPTIONS) {
                    kept.push((path, entry.id()));
                }
            }
            git2::TreeWalkResult::Ok
        })
        .unwrap();
        let mut b = git2::build::TreeUpdateBuilder::new();
        for (path, oid) in &kept {
            b.upsert(path.as_str(), *oid, git2::FileMode::Blob);
        }
        let base = repo.treebuilder(None).unwrap().write().unwrap();
        b.create_updated(repo, &repo.find_tree(base).unwrap())
            .unwrap()
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
        let repo = git2::Repository::init_bare(td.path()).unwrap();

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

        let tree = repo.find_tree(input).unwrap();
        assert_eq!(
            tree.get_path(Path::new("a/x")).unwrap().id(),
            tree.get_path(Path::new("c/x")).unwrap().id(),
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

            let key = git2::Oid::hash_object(git2::ObjectType::Blob, pattern.as_bytes()).unwrap();
            let cp = CompiledPattern::compile(pattern).unwrap();
            assert!(!cp.fallback, "`{pattern}` must not need the fallback");
            let got =
                remove_pattern(&t, input, &cp, key, CompiledPattern::initial_state()).unwrap();
            let want = ground_truth_tree(t.repo(), input, pattern);
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
        let key = git2::Oid::from_str("1234567890123456789012345678901234567890").unwrap();

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
        let repo = git2::Repository::init_bare(td.path()).unwrap();
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
        let repo = git2::Repository::init_bare(td.path()).unwrap();
        let paths: Vec<String> = ["a/f.txt", "b/f.txt"].map(String::from).to_vec();
        let input = make_named_tree(&repo, &paths);
        let tree = repo.find_tree(input).unwrap();
        assert_eq!(
            tree.get_path(Path::new("a")).unwrap().id(),
            tree.get_path(Path::new("b")).unwrap().id()
        );

        let t = open_transaction(&td);
        let pattern = glob::Pattern::new("a/*.txt").unwrap();
        let key = git2::Oid::from_str("abcdef1234567890123456789012345678901234").unwrap();
        let out = remove_pred(
            &t,
            &mut String::new(),
            input,
            &|path, isblob| isblob && pattern.matches_with(path, PATTERN_MATCH_OPTIONS),
            key,
        )
        .unwrap();
        let want = ground_truth_tree(t.repo(), input, "a/*.txt");
        assert_eq!(out, want, "b/f.txt must not be kept via the a/ cache entry");
    }

    // Removing a whole subdirectory must report every file under it as removed. This exercises
    // the "input1 is a tree, input2 is gone" branch of diff_paths, which is only reachable via
    // the recursion for entries present in tree1 but absent from tree2.
    #[test]
    fn diff_paths_reports_removed_subtree() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let tree1 = make_tree(&repo, &["dir/file1", "dir/file2", "kept"]);
        let tree2 = make_tree(&repo, &["kept"]);

        let removed = diff_paths(&repo, tree1, tree2, "").unwrap();
        assert_eq!(
            removed,
            vec![("dir/file1".to_string(), -1), ("dir/file2".to_string(), -1)]
        );

        let added = diff_paths(&repo, tree2, tree1, "").unwrap();
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
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let blob = repo.blob(b"legacy").unwrap();
        let sub_tree = make_tree(&repo, &["dir/a.txt", "dir/b.txt"]);
        let dir = repo
            .find_tree(sub_tree)
            .unwrap()
            .get_name("dir")
            .unwrap()
            .id();
        let input1 = write_raw_tree(
            &repo,
            &[("40000", "dir", dir), ("100664", "legacy.rs", blob)],
        );
        let input2 = make_tree(&repo, &["dir/a.txt"]);

        let t = open_transaction(&td);
        let out = subtract(&t, input1, input2).unwrap();

        let out_tree = t.repo().find_tree(out).unwrap();
        assert_eq!(
            out_tree.get_name("legacy.rs").unwrap().filemode_raw(),
            0o100664,
            "untouched entries must keep their raw mode, like the seeded treebuilder"
        );
        let out_dir = t
            .repo()
            .find_tree(out_tree.get_name("dir").unwrap().id())
            .unwrap();
        assert!(out_dir.get_name("a.txt").is_none(), "matched path removed");
        assert!(out_dir.get_name("b.txt").is_some(), "unmatched path kept");
    }

    // Overlay resolves blob collisions in favor of input1 and takes over input2-only entries
    // with normalized modes, while input1-only entries keep their raw modes (seeded rebuild).
    #[test]
    fn overlay_keeps_input1_on_collision() {
        let td = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_bare(td.path()).unwrap();

        let ours = repo.blob(b"ours").unwrap();
        let theirs = repo.blob(b"theirs").unwrap();
        let input1 = write_raw_tree(&repo, &[("100664", "shared.rs", ours)]);
        let mut b = repo.treebuilder(None).unwrap();
        b.insert("new.rs", theirs, 0o100644).unwrap();
        b.insert("shared.rs", theirs, 0o100644).unwrap();
        let input2 = b.write().unwrap();

        let t = open_transaction(&td);
        let out = overlay(&t, input1, input2).unwrap();

        let out_tree = t.repo().find_tree(out).unwrap();
        let shared = out_tree.get_name("shared.rs").unwrap();
        assert_eq!(shared.id(), ours, "input1 wins blob collisions");
        assert_eq!(
            shared.filemode_raw(),
            0o100644,
            "collision entries are re-inserted with the normalized mode"
        );
        assert_eq!(out_tree.get_name("new.rs").unwrap().id(), theirs);
    }
}
