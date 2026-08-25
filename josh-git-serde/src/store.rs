//! Object-store side of the serde conversion: materialize a [`GitValue`] as git
//! objects over [`gix_object::Write`], and read a tree back over [`gix_object::Find`].

use std::collections::BTreeMap;

use crate::value::GitValue;

/// Write `value` to `out` as git objects, returning the root object's id: a
/// tree oid for [`GitValue::Tree`], a blob oid for [`GitValue::Blob`].
///
/// Every object is written unconditionally; batching and flushing are left to
/// `out`. Tree entries are emitted in canonical git order -- filename bytes,
/// with '/' appended to directory names for the comparison, exactly
/// [`gix_object::tree::Entry`]'s `Ord`.
///
/// Tree entry names must be single path components: non-empty, no '/' or
/// NUL, not "." or "..". Keys produced by the serializer always satisfy
/// this; the check exists because [`GitValue`] is public.
pub fn to_tree_oid(
    out: &impl gix_object::Write,
    value: &GitValue,
) -> anyhow::Result<gix_hash::ObjectId> {
    match value {
        GitValue::Blob(data) => out
            .write_buf(gix_object::Kind::Blob, data)
            .map_err(|e| anyhow::anyhow!("to_tree_oid: {e}")),
        GitValue::Tree(entries) => {
            let mut tree_entries = Vec::with_capacity(entries.len());
            for (name, child) in entries {
                validate_entry_name(name)?;
                let oid = to_tree_oid(out, child)?;
                tree_entries.push(gix_object::tree::Entry {
                    mode: match **child {
                        GitValue::Tree(_) => gix_object::tree::EntryKind::Tree.into(),
                        GitValue::Blob(_) => gix_object::tree::EntryKind::Blob.into(),
                    },
                    filename: name.as_str().into(),
                    oid,
                });
            }
            tree_entries.sort();
            out.write(&gix_object::Tree {
                entries: tree_entries,
            })
            .map_err(|e| anyhow::anyhow!("to_tree_oid: {e}"))
        }
    }
}

/// Read `root` and everything it references from `source` into a
/// [`GitValue`]. `root` may be a tree or a blob; commits, tags, symlinks,
/// executable blobs and submodule entries are rejected -- this crate models
/// plain data, and anything else would be silently mutated on a later write.
///
/// Entry names are taken over as-is; decoding percent-escapes is left to the
/// deserializer. Shared subtrees are materialized repeatedly; the
/// input is expected to be repo-shaped serde data, not arbitrary DAGs.
/// Nesting deeper than [`MAX_TREE_DEPTH`] is rejected rather than risking a
/// stack overflow on adversarial objects, as are duplicate entry names
/// (possible only in hand-crafted trees).
pub fn from_tree_oid(
    source: &impl gix_object::Find,
    root: gix_hash::ObjectId,
) -> anyhow::Result<GitValue> {
    read_tree_oid(source, root, 0)
}

const MAX_TREE_DEPTH: usize = 1024;

fn validate_entry_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(anyhow::anyhow!("to_tree_oid: invalid entry name {name:?}"));
    }
    Ok(())
}

fn read_tree_oid(
    source: &impl gix_object::Find,
    root: gix_hash::ObjectId,
    depth: usize,
) -> anyhow::Result<GitValue> {
    if depth > MAX_TREE_DEPTH {
        return Err(anyhow::anyhow!(
            "from_tree_oid: tree nesting exceeds {MAX_TREE_DEPTH} levels"
        ));
    }

    let mut buf = Vec::new();
    let data = source
        .try_find(&root, &mut buf)
        .map_err(|e| anyhow::anyhow!("from_tree_oid: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("from_tree_oid: object {root} not found"))?;
    let object_hash = data.object_hash;
    match data.kind {
        gix_object::Kind::Blob => Ok(GitValue::Blob(buf)),
        gix_object::Kind::Tree => {
            let tree = gix_object::TreeRef::from_bytes(&buf, object_hash)?;
            let mut entries = BTreeMap::new();
            for entry in &tree.entries {
                let name = String::from_utf8(entry.filename.to_vec()).map_err(|_| {
                    anyhow::anyhow!("from_tree_oid: non-utf8 entry name {:?}", entry.filename)
                })?;
                // `is_blob` also matches executable blobs; accept exactly
                // the regular-file mode this crate writes back.
                if !entry.mode.is_tree() && entry.mode.kind() != gix_object::tree::EntryKind::Blob {
                    return Err(anyhow::anyhow!(
                        "from_tree_oid: entry {:?} has unsupported mode {:?}",
                        entry.filename,
                        entry.mode
                    ));
                }
                let child = read_tree_oid(source, entry.oid.to_owned(), depth + 1)?;
                if entries.insert(name.clone(), Box::new(child)).is_some() {
                    return Err(anyhow::anyhow!(
                        "from_tree_oid: duplicate entry name {name:?}"
                    ));
                }
            }
            Ok(GitValue::Tree(entries))
        }
        kind => Err(anyhow::anyhow!(
            "from_tree_oid: object {root} is a {kind:?}, not a tree or a blob"
        )),
    }
}
