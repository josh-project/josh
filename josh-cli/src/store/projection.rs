use super::*;
pub fn materialize_projections(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    for (remote_name, placement) in &store.config.remotes {
        let upstream_refs = refs_in_superhistory(transaction, remote_name)?;
        let upstream_branches = upstream_refs
            .iter()
            .map(|(branch, _)| branch.clone())
            .collect();
        let backing_prefix = backing_heads_prefix(remote_name);
        reconcile_refs(transaction, &backing_prefix, &upstream_branches)?;
        for (branch, oid) in &upstream_refs {
            transaction.update_ref(
                &(backing_prefix.clone() + branch),
                josh_core::cache::Expected::Any,
                *oid,
                "josh fetch",
            )?;
        }

        let mount_filter =
            josh_core::filter::to_filter(josh_core::filter::Op::Prefix(placement.mount.clone()));
        let (mounted_refs, errors) =
            josh_core::filter_refs(transaction, mount_filter, &upstream_refs);
        if let Some((_, error)) = errors.into_iter().next() {
            bail!("josh mount error: {error}");
        }
        josh_core::filter::as_tree(transaction, mount_filter)?;
        let mounted_branches = mounted_refs
            .iter()
            .filter(|(_, oid)| *oid != gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
            .map(|(branch, _)| branch.clone())
            .collect();
        reconcile_refs(
            transaction,
            &namespace_heads_prefix(ROOT_PROJECTION, remote_name),
            &mounted_branches,
        )?;
        reconcile_refs(
            transaction,
            &format!("refs/remotes/{remote_name}/"),
            &mounted_branches,
        )?;
        for (branch, oid) in &mounted_refs {
            if *oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                continue;
            }
            transaction.update_ref(
                &(namespace_heads_prefix(ROOT_PROJECTION, remote_name) + branch),
                josh_core::cache::Expected::Any,
                *oid,
                "josh mount",
            )?;
            transaction.update_ref(
                &format!("refs/remotes/{remote_name}/{branch}"),
                josh_core::cache::Expected::Any,
                *oid,
                "josh mount",
            )?;
        }
        if let Some(branch) = placement.head.as_deref() {
            set_namespace_head(transaction, ROOT_PROJECTION, remote_name, branch)?;
            transaction.create_symref(
                &format!("refs/remotes/{remote_name}/HEAD"),
                &format!("refs/remotes/{remote_name}/{branch}"),
                "josh mount",
            )?;
        }

        for (target_id, target) in &store.config.projections {
            if target_id == ROOT_PROJECTION {
                continue;
            }
            let filter = josh_core::filter::parse(&target.filter)?;
            let (filtered, errors) = josh_core::filter_refs(transaction, filter, &mounted_refs);
            if let Some((_, error)) = errors.into_iter().next() {
                bail!("josh filter error: {error}");
            }
            let combined_filter = mount_filter.chain(filter);
            let steps = josh_core::filter::flatten_chain(combined_filter);
            let filtered_prefix = crate::remote_ops::step_ref_prefix(steps.len() - 1, &steps);
            josh_core::filter::as_tree(transaction, combined_filter)?;
            let filtered_branches = filtered
                .iter()
                .filter(|(_, oid)| *oid != gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
                .map(|(branch, _)| branch.clone())
                .collect();
            reconcile_refs(
                transaction,
                &namespace_heads_prefix(target_id, remote_name),
                &filtered_branches,
            )?;
            for (branch, oid) in filtered {
                if oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    continue;
                }
                transaction.update_ref(
                    &(namespace_heads_prefix(target_id, remote_name) + &branch),
                    josh_core::cache::Expected::Any,
                    oid,
                    "josh filter",
                )?;
                let filtered_ref = JoshRef::filtered_head(&filtered_prefix, &branch).ref_name();
                transaction.update_ref(
                    &filtered_ref,
                    josh_core::cache::Expected::Any,
                    oid,
                    "josh filter",
                )?;
            }
            if let Some(branch) = placement.head.as_deref() {
                set_namespace_head(transaction, target_id, remote_name, branch)?;
            }
        }
    }
    Ok(())
}
pub(super) fn update_super_ref(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
    updated_remotes: &[String],
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let mut remotes = BTreeMap::new();
    for (name, placement) in &store.config.remotes {
        let config = store.remote_config(name)?;
        remotes.insert(
            name.clone(),
            StoredRemote {
                url: config.url.clone(),
                fetch: config.ref_spec.clone(),
                namespace: placement.namespace.clone(),
                mount: placement.mount.clone(),
                forge: config.forge.map(|forge| forge.to_string()),
                push_url: config.push_url.clone(),
                gerrit_mode: config
                    .filter_with_meta
                    .get_meta("gerrit-mode")
                    .map(|_| config.gerrit_mode.to_string()),
                head: placement.head.clone(),
            },
        );
    }
    let worktrees = store
        .config
        .worktrees
        .values()
        .map(|record| {
            let name = record
                .path
                .strip_prefix(&store.root)
                .unwrap_or(&record.path)
                .to_string_lossy()
                .into_owned();
            (
                name,
                SuperWorktree {
                    projection: record.projection.clone(),
                },
            )
        })
        .collect();
    let config = SuperConfig {
        version: STORE_VERSION,
        remotes,
        projections: store.config.projections.clone(),
        worktrees,
    };
    let config_value = josh_git_serde::to_value(&config)?;
    let config_tree = josh_git_serde::to_tree_oid(odb, &config_value)?;
    let previous = transaction.resolve_ref(SUPER_REF)?;
    let previous_tree = previous
        .map(|oid| josh_core::objects::CommitData::read(odb, oid)?.tree_id())
        .transpose()?;
    let mut tree = josh_core::filter::tree::insert_oid(
        odb,
        josh_core::filter::tree::empty_id(),
        Path::new(SUPER_CONFIG_PATH),
        config_tree,
        0o040000,
    )?;
    let mut present = BTreeSet::new();
    if let Some(previous_tree) = previous_tree {
        for preserved in ["changes", "change-refs", "links"] {
            if let Some(entry) = josh_core::filter::tree::get_path_entry(
                transaction,
                odb,
                previous_tree,
                Path::new(preserved),
            )? {
                present.insert(preserved);
                tree = josh_core::filter::tree::insert_oid(
                    odb,
                    tree,
                    Path::new(preserved),
                    entry.oid,
                    entry.mode.value() as i32,
                )?;
            }
        }
    }
    for required in ["change-refs", "links"] {
        if present.contains(required) {
            continue;
        }
        let marker = BTreeMap::from([("version", "1")]);
        let state = josh_git_serde::to_tree_oid(odb, &josh_git_serde::to_value(&marker)?)?;
        tree =
            josh_core::filter::tree::insert_oid(odb, tree, Path::new(required), state, 0o040000)?;
    }

    for (name, placement) in &store.config.remotes {
        let remote_path = Path::new("remotes").join(name);
        if !updated_remotes.iter().any(|updated| updated == name) {
            if let Some(previous_tree) = previous_tree
                && let Some(remote) = josh_core::filter::tree::get_path_entry(
                    transaction,
                    odb,
                    previous_tree,
                    &remote_path,
                )?
            {
                tree = josh_core::filter::tree::insert_oid(
                    odb,
                    tree,
                    &remote_path,
                    remote.oid,
                    remote.mode.value() as i32,
                )?;
            }
            continue;
        }

        for (branch, oid) in refs_in_namespace(transaction, &placement.namespace, name)? {
            tree = josh_core::filter::tree::insert_oid(
                odb,
                tree,
                &remote_path.join("refs").join("heads").join(branch),
                oid,
                0o160000,
            )?;
        }
        if let Some(branch) = placement.head.as_deref() {
            let oid = transaction
                .resolve_ref(&(namespace_heads_prefix(&placement.namespace, name) + branch))?
                .with_context(|| format!("Remote '{}' HEAD target is missing", name))?;
            tree = josh_core::filter::tree::insert_oid(
                odb,
                tree,
                &remote_path.join("HEAD"),
                oid,
                0o160000,
            )?;
        }
    }

    if let Some(previous) = previous {
        let commit = josh_core::objects::CommitData::read(odb, previous)?;
        if commit.tree_id()? == tree {
            return Ok(());
        }
    }
    let parents: Vec<_> = previous.into_iter().collect();
    let expected = previous.map_or(
        josh_core::cache::Expected::Absent,
        josh_core::cache::Expected::At,
    );
    let signature = josh_core::git::josh_actor_signature()?;
    let commit = josh_core::objects::write_commit(
        odb,
        tree,
        &parents,
        &signature,
        &signature,
        "Update josh superproject state",
    )?;
    transaction.update_ref(SUPER_REF, expected, commit, "josh superproject")?;
    Ok(())
}

fn set_namespace_head(
    transaction: &josh_core::cache::Transaction,
    projection: &str,
    remote: &str,
    branch: &str,
) -> anyhow::Result<()> {
    transaction.create_symref(
        &(namespace_prefix(projection, remote) + "HEAD"),
        &format!("refs/heads/{branch}"),
        "josh filter",
    )
}

fn collect_superhistory_heads(
    transaction: &josh_core::cache::Transaction,
    tree: gix_hash::ObjectId,
    prefix: &str,
    refs: &mut Vec<(String, gix_hash::ObjectId)>,
) -> anyhow::Result<()> {
    for entry in josh_core::objects::read_tree_entries(transaction.odb(), tree)? {
        let name = entry.filename.to_str()?;
        let branch = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode.is_tree() {
            collect_superhistory_heads(transaction, entry.oid, &branch, refs)?;
        } else if entry.mode.is_commit() {
            refs.push((branch, entry.oid));
        }
    }
    Ok(())
}

pub(super) fn refs_in_superhistory(
    transaction: &josh_core::cache::Transaction,
    remote: &str,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let super_commit = transaction
        .resolve_ref(SUPER_REF)?
        .context("Josh store has no synthetic superhistory")?;
    let super_tree =
        josh_core::objects::CommitData::read(transaction.odb(), super_commit)?.tree_id()?;
    let path = Path::new("remotes").join(remote).join("refs").join("heads");
    let heads =
        josh_core::filter::tree::get_path_entry(transaction, transaction.odb(), super_tree, &path)?
            .filter(|entry| entry.mode.is_tree())
            .with_context(|| format!("Remote '{remote}' has no authoritative refs"))?;
    let mut refs = Vec::new();
    collect_superhistory_heads(transaction, heads.oid, "", &mut refs)?;
    Ok(refs)
}

fn refs_in_namespace(
    transaction: &josh_core::cache::Transaction,
    projection: &str,
    remote: &str,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let prefix = namespace_heads_prefix(projection, remote);
    let mut refs = Vec::new();
    transaction.for_each_ref_prefixed(&prefix, |name, oid| {
        if let Some(branch) = name.strip_prefix(&prefix) {
            refs.push((branch.to_owned(), oid));
        }
        Ok(())
    })?;
    Ok(refs)
}

pub fn backing_heads_prefix(remote: &str) -> String {
    JoshRef::remote_head(remote, "").ref_name()
}

fn reconcile_refs(
    transaction: &josh_core::cache::Transaction,
    prefix: &str,
    desired: &BTreeSet<String>,
) -> anyhow::Result<()> {
    let mut stale = Vec::new();
    transaction.for_each_ref_prefixed(prefix, |name, _| {
        if let Some(suffix) = name.strip_prefix(prefix)
            && suffix != "HEAD"
            && !desired.contains(suffix)
        {
            stale.push(name.to_owned());
        }
        Ok(())
    })?;
    for name in stale {
        transaction.delete_ref(&name, josh_core::cache::Expected::Any)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use josh_core::cache::{CacheStack, TransactionContext};

    #[test]
    fn superhistory_recovers_derived_remote_refs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        let mut store = Store::init(&root).unwrap();
        let remote = add_remote(
            &mut store,
            "file:///upstream",
            Path::new("origin"),
            None,
            None,
            None,
        )
        .unwrap();
        store.config.remotes.get_mut(&remote).unwrap().head = Some("main".to_owned());

        let git_dir = store.git_dir();
        let transaction = TransactionContext::new(&git_dir, std::sync::Arc::new(CacheStack::new()))
            .open()
            .unwrap();
        let signature = josh_core::git::josh_actor_signature().unwrap();
        let upstream = josh_core::objects::write_commit(
            transaction.odb(),
            josh_core::filter::tree::empty_id(),
            &[],
            &signature,
            &signature,
            "upstream",
        )
        .unwrap();
        let placement = &store.config.remotes[&remote];
        let staging = namespace_heads_prefix(&placement.namespace, &remote) + "main";
        transaction
            .update_ref(
                &staging,
                josh_core::cache::Expected::Any,
                upstream,
                "test fetch",
            )
            .unwrap();
        let changes = josh_changes::ChangesRef::Local {
            branch: "main".to_owned(),
        };
        transaction
            .update_ref(
                &changes.ref_name(),
                josh_core::cache::Expected::Any,
                upstream,
                "test change state",
            )
            .unwrap();
        josh_changes::sync_change_refs_into_superhistory(&transaction).unwrap();
        let link = josh_view::View {
            filter: josh_core::filter::parse(":/").unwrap(),
            link: Some(josh_view::Link {
                url: "file:///linked".to_owned(),
                tracked_ref: "refs/heads/main".to_owned(),
                forge: None,
            }),
        };
        let link_blob =
            josh_core::objects::write_blob(transaction.odb(), link.to_file_string().as_bytes())
                .unwrap();
        let link_tree = josh_core::filter::tree::insert_oid(
            transaction.odb(),
            josh_core::filter::tree::empty_id(),
            Path::new("link.josh"),
            link_blob,
            0o100644,
        )
        .unwrap();
        let link_commit = josh_core::objects::write_commit(
            transaction.odb(),
            link_tree,
            &[],
            &signature,
            &signature,
            "test link",
        )
        .unwrap();
        transaction
            .update_ref(
                &josh_view::link_ref_name("linked"),
                josh_core::cache::Expected::Any,
                link_commit,
                "test link",
            )
            .unwrap();
        josh_changes::sync_link_refs_into_superhistory(&transaction).unwrap();

        update_super_ref(&store, &transaction, std::slice::from_ref(&remote)).unwrap();
        materialize_projections(&store, &transaction).unwrap();
        let backing = JoshRef::remote_head(&remote, "main").ref_name();
        let projected = namespace_heads_prefix(ROOT_PROJECTION, &remote) + "main";
        assert_eq!(transaction.resolve_ref(&backing).unwrap(), Some(upstream));
        assert_eq!(transaction.resolve_ref(&projected).unwrap(), Some(upstream));
        transaction
            .delete_ref(&changes.ref_name(), josh_core::cache::Expected::Any)
            .unwrap();
        assert!(josh_changes::materialize_change_refs_from_superhistory(&transaction).unwrap());
        assert_eq!(
            transaction.resolve_ref(&changes.ref_name()).unwrap(),
            Some(upstream)
        );
        transaction
            .delete_ref(
                &josh_view::link_ref_name("linked"),
                josh_core::cache::Expected::Any,
            )
            .unwrap();
        assert!(josh_changes::materialize_link_refs_from_superhistory(&transaction).unwrap());
        assert_eq!(
            josh_view::read_view(&transaction, "linked").unwrap(),
            Some(link)
        );

        let staging_only = josh_core::objects::write_commit(
            transaction.odb(),
            josh_core::filter::tree::empty_id(),
            &[upstream],
            &signature,
            &signature,
            "not committed to superhistory",
        )
        .unwrap();
        transaction
            .update_ref(
                &staging,
                josh_core::cache::Expected::Any,
                staging_only,
                "staging changed",
            )
            .unwrap();
        transaction
            .delete_ref(&backing, josh_core::cache::Expected::Any)
            .unwrap();
        transaction
            .delete_ref(&projected, josh_core::cache::Expected::Any)
            .unwrap();

        materialize_projections(&store, &transaction).unwrap();
        assert_eq!(transaction.resolve_ref(&backing).unwrap(), Some(upstream));
        assert_eq!(transaction.resolve_ref(&projected).unwrap(), Some(upstream));

        transaction
            .delete_ref(&backing, josh_core::cache::Expected::Any)
            .unwrap();
        transaction
            .delete_ref(&projected, josh_core::cache::Expected::Any)
            .unwrap();
        transaction.flush_mem_odb().unwrap();
        drop(transaction);

        let sidecars = store.git_dir().join("josh").join("remotes");
        std::fs::create_dir_all(&sidecars).unwrap();
        std::fs::write(sidecars.join("stale.josh"), "stale").unwrap();
        let loaded = Store::load(root).unwrap();
        assert_eq!(loaded.config.remotes[&remote].head.as_deref(), Some("main"));
        assert!(!sidecars.exists());
    }
}
