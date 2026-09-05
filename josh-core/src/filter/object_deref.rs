use super::{Filter, Op, Rewrite, apply, legalize_stored, to_filter, tree};
use crate::{cache, git, history, objects};
use std::path::Path;

fn legalize_object_derefs(filter: Filter) -> Filter {
    to_filter(match super::to_op(filter) {
        Op::ObjectDeref(path) => Op::ObjectRef(path),
        Op::Compose(filters) => {
            Op::Compose(filters.into_iter().map(legalize_object_derefs).collect())
        }
        Op::Chain(filters) => Op::Chain(filters.into_iter().map(legalize_object_derefs).collect()),
        Op::Subtract(a, b) => Op::Subtract(legalize_object_derefs(a), legalize_object_derefs(b)),
        Op::Exclude(filter) => Op::Exclude(legalize_object_derefs(filter)),
        Op::Select(filter) => Op::Select(legalize_object_derefs(filter)),
        Op::Pin(filter) => Op::Pin(legalize_object_derefs(filter)),
        Op::Starlark(path, filter) => Op::Starlark(path, legalize_object_derefs(filter)),
        Op::TreeId(path, filter) => Op::TreeId(path, legalize_object_derefs(filter)),
        Op::Unapply(target, filter) => Op::Unapply(target, legalize_object_derefs(filter)),
        Op::Rev(filters) => Op::Rev(
            filters
                .into_iter()
                .map(|(match_op, filter)| (match_op, legalize_object_derefs(filter)))
                .collect(),
        ),
        Op::Meta(meta, filter) => Op::Meta(meta, legalize_object_derefs(filter)),
        op => op,
    })
}

/// Apply the filter with dereferences replaced by references, then retain only
/// the resulting gitlinks. Their paths are the destinations in the filtered tree,
/// including prefixes and duplicate target objects at different paths.
fn object_reference_tree(
    transaction: &cache::Transaction,
    filter: Filter,
    input_tree: gix_hash::ObjectId,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let odb = transaction.odb();
    let reader = tree::read_tree(transaction, odb, input_tree)?;
    let filter = legalize_stored(transaction, odb, filter, input_tree, &reader)?;
    let pointer_filter = legalize_object_derefs(filter);
    if pointer_filter == filter {
        return Ok(None);
    }

    let pointer_tree =
        apply(transaction, pointer_filter, Rewrite::from_tree(input_tree))?.tree_id();
    // `remove_pred` keeps every non-gitlink; subtracting it leaves only gitlinks.
    let without_gitlinks = tree::remove_pred(
        transaction,
        &mut String::new(),
        pointer_tree,
        &|_, _| true,
        objects::hash_blob(b"object-reference-tree"),
    )?;
    Ok(Some(tree::subtract(
        transaction,
        pointer_tree,
        without_gitlinks,
    )?))
}

fn gitlink_entries(
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<(std::path::PathBuf, gix_hash::ObjectId)>> {
    let mut entries = Vec::new();
    objects::walk_tree_preorder(odb, tree, &mut |parent, entry| {
        if entry.mode.is_commit() {
            let name = std::str::from_utf8(entry.filename)?;
            let path = if parent.is_empty() {
                name.into()
            } else {
                Path::new(parent).join(name)
            };
            entries.push((path, entry.oid.to_owned()));
        }
        Ok(())
    })?;
    Ok(entries)
}

fn commit_reference_at_path(
    transaction: &cache::Transaction,
    odb: &josh_memodb::Odb,
    tree: gix_hash::ObjectId,
    path: &Path,
) -> Option<gix_hash::ObjectId> {
    tree::get_path_entry(transaction, odb, tree, path)
        .ok()
        .flatten()
        .filter(|entry| entry.mode.is_commit())
        .and_then(|entry| {
            matches!(odb.try_kind(entry.oid), Ok(Some(gix_object::Kind::Commit)))
                .then_some(entry.oid)
        })
}

pub(super) fn append_parents(
    transaction: &cache::Transaction,
    commit: &objects::CommitData,
    filter: Filter,
    parent_filter: Option<Filter>,
    filtered_tree: gix_hash::ObjectId,
    original_target: gix_hash::ObjectId,
    filtered_parents: &mut Vec<gix_hash::ObjectId>,
) -> anyhow::Result<bool> {
    let odb = transaction.odb();
    let Some(references) = object_reference_tree(transaction, filter, commit.tree_id()?)? else {
        return Ok(true);
    };
    let parent_references = commit
        .first_parent_id()
        .zip(parent_filter)
        .map(|(parent, parent_filter)| {
            object_reference_tree(transaction, parent_filter, git::read_tree_id(odb, parent)?)
        })
        .transpose()?
        .flatten();
    let mut pending = Vec::new();
    let mut ready = true;

    for (path, new_oid) in gitlink_entries(odb, references)? {
        if !matches!(odb.try_kind(new_oid), Ok(Some(gix_object::Kind::Commit))) {
            continue;
        }
        let referenced_tree = git::read_tree_id(odb, new_oid)?;
        let appears_in_output = tree::get_path_entry(transaction, odb, filtered_tree, &path)
            .ok()
            .flatten()
            .is_some_and(|entry| entry.mode.is_tree() && entry.oid == referenced_tree);
        if !appears_in_output {
            continue;
        }

        let old_oid = parent_references
            .and_then(|tree| commit_reference_at_path(transaction, odb, tree, &path))
            .unwrap_or_else(|| gix_hash::ObjectId::null(gix_hash::Kind::Sha1));
        if old_oid == new_oid {
            continue;
        }

        let path_filter = to_filter(Op::Subdir(path));
        if original_target != gix_hash::ObjectId::null(gix_hash::Kind::Sha1)
            && transaction.get(path_filter, original_target)?.is_none()
        {
            ready = false;
        }
        pending.push((path_filter, old_oid, new_oid));
    }

    if !ready {
        return Ok(false);
    }

    for (path_filter, old_oid, new_oid) in pending {
        let referenced_history = history::unapply_filter(
            transaction,
            path_filter,
            original_target,
            old_oid,
            new_oid,
            history::OrphansMode::Keep,
            None,
        )?;
        if referenced_history != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
            filtered_parents.push(referenced_history);
        }
    }

    Ok(true)
}
