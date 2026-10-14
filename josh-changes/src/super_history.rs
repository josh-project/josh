use crate::change::Change;
use crate::layout::SuperRefData;
use crate::refs::{ChangesRef, JoshRef, LOCAL_CHANGES_REF_PREFIX, REMOTE_REF_PREFIX, SUPER_REF};
use crate::store::DiffData;
use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::objects;

fn change_ref_links(
    transaction: &Transaction,
) -> anyhow::Result<Vec<(ChangesRef, gix_hash::ObjectId)>> {
    let mut refs = Vec::new();
    transaction.for_each_ref_prefixed(LOCAL_CHANGES_REF_PREFIX, |name, oid| {
        let branch = &name[LOCAL_CHANGES_REF_PREFIX.len()..];
        if !branch.is_empty() {
            refs.push((
                ChangesRef::Local {
                    branch: branch.to_owned(),
                },
                oid,
            ));
        }
        Ok(())
    })?;
    transaction.for_each_ref_prefixed(REMOTE_REF_PREFIX, |name, oid| {
        let rest = &name[REMOTE_REF_PREFIX.len()..];
        if let Some((remote, branch)) = rest.split_once("/changes/")
            && !remote.is_empty()
            && !branch.is_empty()
        {
            refs.push((
                ChangesRef::Remote {
                    remote: remote.to_owned(),
                    branch: branch.to_owned(),
                },
                oid,
            ));
        }
        Ok(())
    })?;
    Ok(refs)
}

fn overlay_data<T: serde::Serialize>(
    transaction: &Transaction,
    base: gix_hash::ObjectId,
    data: &T,
) -> anyhow::Result<(gix_hash::ObjectId, bool)> {
    let value = josh_git_serde::to_value(data)?;
    let update = josh_git_serde::to_tree_oid(transaction.odb(), &value)?;
    let merged = tree::overlay(transaction, update, base)?;
    Ok((merged, merged != base))
}

fn super_base(
    transaction: &Transaction,
) -> anyhow::Result<(Option<gix_hash::ObjectId>, gix_hash::ObjectId)> {
    let previous = transaction.resolve_ref(SUPER_REF)?;
    let tree = match previous {
        Some(previous) => objects::CommitData::read(transaction.odb(), previous)?.tree_id()?,
        None => tree::empty_id(),
    };
    Ok((previous, tree))
}

fn update_super_ref(
    transaction: &Transaction,
    previous: Option<gix_hash::ObjectId>,
    tree: gix_hash::ObjectId,
    message: &str,
) -> anyhow::Result<()> {
    let signature = josh_core::git::josh_actor_signature()?;
    let parents: Vec<_> = previous.into_iter().collect();
    let commit = objects::write_commit(
        transaction.odb(),
        tree,
        &parents,
        &signature,
        &signature,
        message,
    )?;
    transaction.update_ref(
        SUPER_REF,
        previous.map_or(Expected::Absent, Expected::At),
        commit,
        message,
    )
}

/// Commit every materialized changes ref into the superhistory as a gitlink.
pub fn sync_change_refs_into_superhistory(transaction: &Transaction) -> anyhow::Result<()> {
    let (previous, mut super_tree) = super_base(transaction)?;
    let marker = std::collections::BTreeMap::from([("version", "1")]);
    let mut links =
        josh_git_serde::to_tree_oid(transaction.odb(), &josh_git_serde::to_value(&marker)?)?;
    for (scope, oid) in change_ref_links(transaction)? {
        let path = match scope {
            ChangesRef::Local { branch } => {
                std::path::Path::new("local").join(josh_git_serde::encode_key(&branch))
            }
            ChangesRef::Remote { remote, branch } => std::path::Path::new("remotes")
                .join(josh_git_serde::encode_key(&remote))
                .join(josh_git_serde::encode_key(&branch)),
        };
        links = tree::insert_oid(transaction.odb(), links, &path, oid, 0o160000)?;
    }
    super_tree = tree::insert_oid(
        transaction.odb(),
        super_tree,
        std::path::Path::new("change-refs"),
        links,
        0o040000,
    )?;
    if let Some(previous) = previous
        && objects::CommitData::read(transaction.odb(), previous)?.tree_id()? == super_tree
    {
        return Ok(());
    }
    update_super_ref(
        transaction,
        previous,
        super_tree,
        "Update authoritative josh change refs",
    )
}

fn linked_entries(
    transaction: &Transaction,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let mut entries = Vec::new();
    for entry in objects::read_tree_entries(transaction.odb(), tree)? {
        if entry.mode.is_commit() {
            entries.push((
                josh_git_serde::decode_key(std::str::from_utf8(entry.filename.as_ref())?)?,
                entry.oid,
            ));
        }
    }
    Ok(entries)
}

/// Rebuild materialized changes refs from the authoritative superhistory.
///
/// Returns `false` for stores created before the authoritative `change-refs`
/// tree existed, allowing the caller to bootstrap it from existing refs.
pub fn materialize_change_refs_from_superhistory(
    transaction: &Transaction,
) -> anyhow::Result<bool> {
    let Some(super_commit) = transaction.resolve_ref(SUPER_REF)? else {
        return Ok(false);
    };
    let super_tree = objects::CommitData::read(transaction.odb(), super_commit)?.tree_id()?;
    let Some(root) = tree::get_path_entry(
        transaction,
        transaction.odb(),
        super_tree,
        std::path::Path::new("change-refs"),
    )?
    .filter(|entry| entry.mode.is_tree()) else {
        return Ok(false);
    };

    let mut desired = std::collections::BTreeMap::new();
    if let Some(local) = tree::get_path_entry(
        transaction,
        transaction.odb(),
        root.oid,
        std::path::Path::new("local"),
    )?
    .filter(|entry| entry.mode.is_tree())
    {
        for (branch, oid) in linked_entries(transaction, local.oid)? {
            desired.insert(ChangesRef::Local { branch }.ref_name(), oid);
        }
    }
    if let Some(remotes) = tree::get_path_entry(
        transaction,
        transaction.odb(),
        root.oid,
        std::path::Path::new("remotes"),
    )?
    .filter(|entry| entry.mode.is_tree())
    {
        for remote in objects::read_tree_entries(transaction.odb(), remotes.oid)? {
            if !remote.mode.is_tree() {
                continue;
            }
            let remote_name =
                josh_git_serde::decode_key(std::str::from_utf8(remote.filename.as_ref())?)?;
            for (branch, oid) in linked_entries(transaction, remote.oid)? {
                desired.insert(
                    ChangesRef::Remote {
                        remote: remote_name.clone(),
                        branch,
                    }
                    .ref_name(),
                    oid,
                );
            }
        }
    }

    let mut materialized = Vec::new();
    transaction.for_each_ref_prefixed(LOCAL_CHANGES_REF_PREFIX, |name, _| {
        materialized.push(name.to_owned());
        Ok(())
    })?;
    transaction.for_each_ref_prefixed(REMOTE_REF_PREFIX, |name, _| {
        if name[REMOTE_REF_PREFIX.len()..].contains("/changes/") {
            materialized.push(name.to_owned());
        }
        Ok(())
    })?;
    for name in materialized {
        if !desired.contains_key(&name) {
            transaction.delete_ref(&name, Expected::Any)?;
        }
    }
    for (name, oid) in desired {
        transaction.update_ref(&name, Expected::Any, oid, "materialize josh change ref")?;
    }
    Ok(true)
}

/// Commit every materialized link ref into the superhistory as a gitlink.
pub fn sync_link_refs_into_superhistory(transaction: &Transaction) -> anyhow::Result<()> {
    let (previous, mut super_tree) = super_base(transaction)?;
    let marker = std::collections::BTreeMap::from([("version", "1")]);
    let mut links =
        josh_git_serde::to_tree_oid(transaction.odb(), &josh_git_serde::to_value(&marker)?)?;
    transaction.for_each_ref_prefixed(josh_view::LINKS_REF_PREFIX, |name, oid| {
        let id = &name[josh_view::LINKS_REF_PREFIX.len()..];
        if !id.is_empty() {
            links = tree::insert_oid(
                transaction.odb(),
                links,
                std::path::Path::new(&josh_git_serde::encode_key(id)),
                oid,
                0o160000,
            )?;
        }
        Ok(())
    })?;
    super_tree = tree::insert_oid(
        transaction.odb(),
        super_tree,
        std::path::Path::new("links"),
        links,
        0o040000,
    )?;
    if let Some(previous) = previous
        && objects::CommitData::read(transaction.odb(), previous)?.tree_id()? == super_tree
    {
        return Ok(());
    }
    update_super_ref(
        transaction,
        previous,
        super_tree,
        "Update authoritative josh links",
    )
}

/// Rebuild materialized link refs from the authoritative superhistory.
pub fn materialize_link_refs_from_superhistory(transaction: &Transaction) -> anyhow::Result<bool> {
    let Some(super_commit) = transaction.resolve_ref(SUPER_REF)? else {
        return Ok(false);
    };
    let super_tree = objects::CommitData::read(transaction.odb(), super_commit)?.tree_id()?;
    let Some(root) = tree::get_path_entry(
        transaction,
        transaction.odb(),
        super_tree,
        std::path::Path::new("links"),
    )?
    .filter(|entry| entry.mode.is_tree()) else {
        return Ok(false);
    };
    let desired: std::collections::BTreeMap<_, _> = linked_entries(transaction, root.oid)?
        .into_iter()
        .map(|(id, oid)| (josh_view::link_ref_name(&id), oid))
        .collect();
    let mut materialized = Vec::new();
    transaction.for_each_ref_prefixed(josh_view::LINKS_REF_PREFIX, |name, _| {
        materialized.push(name.to_owned());
        Ok(())
    })?;
    for name in materialized {
        if !desired.contains_key(&name) {
            transaction.delete_ref(&name, Expected::Any)?;
        }
    }
    for (name, oid) in desired {
        transaction.update_ref(&name, Expected::Any, oid, "materialize josh link")?;
    }
    Ok(true)
}

/// Record upstream change revisions in the store-wide change index.
///
/// The index remains linear and does not parent upstream history. A separate
/// per-remote-branch retention history keeps every recorded revision reachable
/// through anchor parents.
pub fn record_upstream_change_revisions(
    transaction: &Transaction,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
    remote: &str,
    branch: &str,
) -> anyhow::Result<Vec<Change>> {
    let mut changes = crate::change::split_changes(
        transaction,
        crate::change::get_changes(transaction, tip, base)?,
    )?;
    changes.sort_by(|left, right| left.id().cmp(&right.id()));
    if changes.is_empty() {
        return Ok(changes);
    }

    let (previous_super, mut super_tree) = super_base(transaction)?;
    let original_tree = super_tree;
    let signature = josh_core::git::josh_actor_signature()?;
    let mut anchors = Vec::new();

    for change in &changes {
        let change_id = change
            .id()
            .ok_or_else(|| anyhow::anyhow!("commit {} has no Change-Id", change.commit()))?;
        let revision = DiffData {
            commit: change.commit().to_string(),
            base: change.base().to_string(),
        };
        let projection = SuperRefData::projection(change_id, remote, branch, revision);
        let projection_changed;
        (super_tree, projection_changed) = overlay_data(transaction, super_tree, &projection)?;
        if projection_changed {
            let anchor = objects::write_commit(
                transaction.odb(),
                tree::empty_id(),
                &[change.commit()],
                &signature,
                &signature,
                "Retain josh change revision",
            )?;
            anchors.push(anchor);
        }
    }

    if super_tree == original_tree {
        return Ok(changes);
    }

    if !anchors.is_empty() {
        let retention_ref = JoshRef::change_retention(remote, branch).ref_name();
        let previous_retention = transaction.resolve_ref(&retention_ref)?;
        let mut retention_parents: Vec<_> = previous_retention.into_iter().collect();
        retention_parents.extend(anchors);
        let retention = objects::write_commit(
            transaction.odb(),
            tree::empty_id(),
            &retention_parents,
            &signature,
            &signature,
            "Retain josh change revisions",
        )?;
        transaction.update_ref(
            &retention_ref,
            previous_retention.map_or(Expected::Absent, Expected::At),
            retention,
            "josh change retention",
        )?;
    }

    update_super_ref(
        transaction,
        previous_super,
        super_tree,
        "Update store-wide josh changes",
    )?;
    Ok(changes)
}
