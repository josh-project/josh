use super::*;
pub fn add_remote(
    store: &mut Store,
    url: &str,
    mount_path: &Path,
    forge: Option<Forge>,
    push_url: Option<&str>,
    gerrit_mode: Option<GerritMode>,
) -> anyhow::Result<String> {
    let (filter, mount, name) = parse_mount_path(mount_path)?;
    for placement in store.config.remotes.values() {
        if mount.starts_with(&placement.mount) || placement.mount.starts_with(&mount) {
            bail!(
                "Remote mount '{}' overlaps existing mount '{}'",
                mount.display(),
                placement.mount.display()
            );
        }
    }
    if store.config.remotes.contains_key(&name) {
        bail!("A remote already exists at '{}'", mount.display());
    }
    check_remote_name(&name)?;

    let namespace = format!("remote-{name}");
    let refspec = format!(
        "+refs/heads/*:{}*",
        namespace_heads_prefix(&namespace, &name)
    );
    let remote_config =
        crate::config::build_remote_config(url, &filter, &refspec, forge, push_url, gerrit_mode)?;
    store
        .config
        .remote_configs
        .insert(name.clone(), remote_config);
    store.config.remotes.insert(
        name.clone(),
        RemotePlacement {
            namespace,
            mount,
            head: None,
        },
    );
    for worktree in store.config.worktrees.values() {
        if worktree.path.exists() {
            configure_worktree_remote(store, &worktree.path, &worktree.projection, &name)?;
        }
    }
    Ok(name)
}

pub fn rollback_remote(
    store: &mut Store,
    name: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let Some(placement) = store.config.remotes.remove(name) else {
        return Ok(());
    };
    store.config.remote_configs.remove(name);
    for worktree in store.config.worktrees.values() {
        if worktree.path.exists() {
            let _ = git_output(&worktree.path, ["remote", "remove", name]);
        }
    }

    let mut prefixes = vec![
        namespace_prefix(&placement.namespace, name),
        backing_heads_prefix(name),
        format!("refs/remotes/{name}/"),
    ];
    prefixes.extend(
        store
            .config
            .projections
            .keys()
            .map(|projection| namespace_prefix(projection, name)),
    );
    for prefix in prefixes {
        let mut refs = Vec::new();
        transaction.for_each_ref_prefixed(&prefix, |refname, _| {
            refs.push(refname.to_owned());
            Ok(())
        })?;
        for refname in refs {
            transaction.delete_ref(&refname, josh_core::cache::Expected::Any)?;
        }
    }

    update_super_ref(store, transaction, &[])?;
    transaction.flush_mem_odb()
}

pub fn fetch(
    store: &mut Store,
    context: &ExecutionContext,
    selected_remote: Option<&str>,
) -> anyhow::Result<()> {
    fetch_impl(store, context, selected_remote, true)
}

pub fn fetch_quiet(
    store: &mut Store,
    context: &ExecutionContext,
    selected_remote: Option<&str>,
) -> anyhow::Result<()> {
    fetch_impl(store, context, selected_remote, false)
}

fn fetch_impl(
    store: &mut Store,
    context: &ExecutionContext,
    selected_remote: Option<&str>,
    announce: bool,
) -> anyhow::Result<()> {
    let names: Vec<String> = match selected_remote {
        Some(name) => {
            if !store.config.remotes.contains_key(name) {
                bail!("Unknown remote '{}'", name);
            }
            vec![name.to_owned()]
        }
        None => store.config.remotes.keys().cloned().collect(),
    };

    for name in &names {
        fetch_upstream(store, context, name, announce)?;
    }
    let transaction = context.transaction();
    update_super_ref(store, transaction, &names)?;
    materialize_projections(store, transaction)?;
    transaction.flush_mem_odb()?;
    fetch_worktrees(store, transaction)?;
    Ok(())
}
struct StoreRelay {
    remote: String,
    refspecs: Vec<String>,
    filter: josh_core::filter::Filter,
}
fn select_worktree_remote(
    store: &Store,
    selected_remote: Option<&str>,
    worktree: &Path,
) -> anyhow::Result<(String, String)> {
    let current_branch = ensure_success(
        git_output(worktree, ["symbolic-ref", "--quiet", "--short", "HEAD"])?,
        "Detached HEAD requires an explicit branch",
    )?
    .trim()
    .to_owned();
    let remote = match selected_remote {
        Some(remote) => remote.to_owned(),
        None if store.config.remotes.len() == 1 => store
            .config
            .remotes
            .keys()
            .next()
            .expect("length checked")
            .clone(),
        None => {
            let key = format!("branch.{current_branch}.remote");
            let configured = git_output(worktree, ["config", "--get", &key])?;
            if configured.status.success() {
                String::from_utf8(configured.stdout)?.trim().to_owned()
            } else {
                bail!("Multiple remotes are configured; specify a remote");
            }
        }
    };
    if !store.config.remotes.contains_key(&remote) {
        bail!("Unknown remote '{}'", remote);
    }
    Ok((remote, current_branch))
}

fn prepare_relay(
    store: &Store,
    selected_remote: Option<&str>,
    requested_refspecs: &[String],
    force: bool,
) -> anyhow::Result<StoreRelay> {
    let worktree = std::env::current_dir()?;
    let projection = current_projection(&worktree)?;
    let (remote, current_branch) = select_worktree_remote(store, selected_remote, &worktree)?;
    let placement = store
        .config
        .remotes
        .get(&remote)
        .ok_or_else(|| anyhow!("Unknown remote '{}'", remote))?;
    let current = store
        .config
        .projections
        .get(&projection)
        .ok_or_else(|| anyhow!("Unknown worktree projection '{}'", projection))?;
    let mount_filter =
        josh_core::filter::to_filter(josh_core::filter::Op::Prefix(placement.mount.clone()));
    let filter = mount_filter.chain(josh_core::filter::parse(&current.filter)?);

    let requested = if requested_refspecs.is_empty() {
        vec![current_branch.clone()]
    } else {
        requested_refspecs.to_vec()
    };
    let mut parked = Vec::with_capacity(requested.len());
    for refspec in requested {
        let (source, destination) = match refspec.split_once(':') {
            Some((source, destination)) => (source.to_owned(), destination.to_owned()),
            None => {
                let destination = if refspec == "HEAD" {
                    current_branch.clone()
                } else {
                    refspec.clone()
                };
                (refspec, destination)
            }
        };
        let destination = destination
            .strip_prefix("refs/heads/")
            .unwrap_or(&destination)
            .to_owned();
        if destination.is_empty() || destination.starts_with("refs/") {
            bail!("Only branch destinations are supported by josh push");
        }
        let mut command = Command::new("git");
        command.current_dir(&worktree).arg("push");
        if force {
            command.arg("--force");
        }
        let store_refspec = format!("{source}:refs/heads/{destination}");
        let output = command.arg(&remote).arg(&store_refspec).output()?;
        ensure_success(output, "Failed to park branch in the store")?;
        parked.push(format!(
            "{}{}:refs/heads/{}",
            namespace_heads_prefix(&projection, &remote),
            destination,
            destination
        ));
    }

    Ok(StoreRelay {
        remote,
        refspecs: parked,
        filter,
    })
}

pub fn push(
    store: &Store,
    args: &crate::commands::push::PushArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let relay = prepare_relay(store, args.remote.as_deref(), &args.refspecs, args.force)?;
    let config = store.remote_config(&relay.remote)?;
    crate::commands::push::handle_store_push(
        args,
        &relay.remote,
        config,
        &relay.refspecs,
        relay.filter,
        transaction,
    )
}

pub fn import_worktree_refs(
    remote: &str,
    prefixes: &[&str],
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    let projection = current_projection(&worktree)?;
    for prefix in prefixes {
        let refs = ensure_success(
            git_output(&worktree, ["for-each-ref", "--format=%(refname)", prefix])?,
            "Failed to list shared worktree refs",
        )?;
        for refname in refs.lines() {
            let refspec = format!("+{refname}:{refname}");
            let output = git_output(&worktree, ["push", "--quiet", remote, &refspec])?;
            ensure_success(output, "Failed to copy worktree state into the store")?;
            let namespaced = namespace_prefix(&projection, remote) + refname;
            let oid = transaction
                .resolve_ref(&namespaced)?
                .ok_or_else(|| anyhow!("Ref '{}' was not received by the store", refname))?;
            transaction.update_ref(
                refname,
                josh_core::cache::Expected::Any,
                oid,
                "josh worktree state",
            )?;
        }
    }
    Ok(())
}

fn clear_imported_change_refs(transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    let mut refs = Vec::new();
    transaction.for_each_ref_prefixed("refs/josh/changes/", |refname, _| {
        refs.push(refname.to_owned());
        Ok(())
    })?;
    transaction.for_each_ref_prefixed("refs/josh/remotes/", |refname, _| {
        if refname
            .strip_prefix("refs/josh/remotes/")
            .is_some_and(|name| name.contains("/changes/"))
        {
            refs.push(refname.to_owned());
        }
        Ok(())
    })?;
    for refname in refs {
        transaction.delete_ref(&refname, josh_core::cache::Expected::Any)?;
    }
    Ok(())
}

pub fn sync_worktree_change_metadata(
    store: &Store,
    cache_policy: CachePolicy,
) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    let (remote, _) = select_worktree_remote(store, None, &worktree)?;
    let git_dir = store.git_dir();
    let context = cache_policy.open(&git_dir, &git_dir, true, false)?;
    let transaction = context.transaction();
    import_worktree_refs(
        &remote,
        &["refs/josh/changes/", "refs/josh/remotes/"],
        transaction,
    )?;
    josh_changes::sync_change_refs_into_superhistory(transaction)?;
    clear_imported_change_refs(transaction)?;
    josh_changes::materialize_change_refs_from_superhistory(transaction)?;
    transaction.flush_mem_odb()
}

fn upstream_remote_head(
    store: &Store,
    remote: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<(String, gix_hash::ObjectId)> {
    let placement = store
        .config
        .remotes
        .get(remote)
        .with_context(|| format!("Unknown remote '{}'", remote))?;
    let branch = placement
        .head
        .clone()
        .with_context(|| format!("Remote '{}' has no default branch", remote))?;
    let oid = refs_in_superhistory(transaction, remote)?
        .into_iter()
        .find_map(|(candidate, oid)| (candidate == branch).then_some(oid))
        .with_context(|| format!("Remote '{}' default branch was not fetched", remote))?;
    Ok((branch, oid))
}
pub fn discover_submodule_remotes(
    store: &Store,
    remote: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<Vec<crate::commands::submodules::SubmoduleRemote>> {
    let config = store.remote_config(remote)?;
    let (_, commit) = upstream_remote_head(store, remote, transaction)?;
    crate::commands::submodules::submodule_remotes(transaction, commit, &config.url)
}
pub fn derive_submodule_filter(
    store: &Store,
    remote: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<String> {
    let config = store.remote_config(remote)?;
    let placement = store
        .config
        .remotes
        .get(remote)
        .with_context(|| format!("Unknown remote '{}'", remote))?;
    let (branch, commit) = upstream_remote_head(store, remote, transaction)?;
    let inline_filter = crate::commands::submodules::add_submodule_links_at_commit(
        transaction,
        &config.url,
        commit,
        Some(&branch),
    )?;
    transaction.flush_mem_odb()?;
    let select_mount =
        josh_core::filter::to_filter(josh_core::filter::Op::Subdir(placement.mount.clone()));
    Ok(josh_core::filter::spec(select_mount.chain(inline_filter)))
}

pub fn publish(
    store: &Store,
    args: &crate::commands::push::PublishArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let relay = prepare_relay(store, args.remote.as_deref(), &args.refspecs, true)?;
    import_worktree_refs(
        &relay.remote,
        &["refs/josh/changes/", "refs/josh/remotes/"],
        transaction,
    )?;
    let worktree = std::env::current_dir()?;
    let author = ensure_success(
        git_output(&worktree, ["config", "--get", "user.email"])?,
        "user.email is required for publishing changes",
    )?;
    let config = store.remote_config(&relay.remote)?;
    let result = crate::commands::push::handle_store_publish(
        args,
        &relay.remote,
        config,
        &relay.refspecs,
        relay.filter,
        author.trim(),
        transaction,
    );
    if result.is_ok() {
        josh_changes::sync_change_refs_into_superhistory(transaction)?;
    }
    clear_imported_change_refs(transaction)?;
    josh_changes::materialize_change_refs_from_superhistory(transaction)?;
    result
}
pub fn pull(
    store: &mut Store,
    args: &crate::commands::pull::PullArgs,
    context: &ExecutionContext,
) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    current_projection(&worktree)?;
    let (remote, _) = select_worktree_remote(store, args.remote.as_deref(), &worktree)?;
    fetch(store, context, Some(&remote))?;
    let mut command = Command::new("git");
    command
        .current_dir(&worktree)
        .args(["pull", "--rebase", "--autostash", &remote]);
    if args.rref != "HEAD" {
        command.arg(&args.rref);
    }
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "Failed to integrate fetched history: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

pub fn pull_changes(
    store: &mut Store,
    args: &crate::commands::pull::PullArgs,
    context: &ExecutionContext,
) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    let (remote, _) = select_worktree_remote(store, args.remote.as_deref(), &worktree)?;
    fetch(store, context, Some(&remote))?;
    let paths = josh_core::git::discover_repository_paths()?;
    let worktree_context =
        context
            .cache_policy()
            .open(&paths.workdir_or_gitdir, &paths.common_dir, true, false)?;
    crate::commands::pull::integrate_fetched(&remote, worktree_context.transaction())
}

fn fetch_upstream(
    store: &mut Store,
    context: &ExecutionContext,
    name: &str,
    announce: bool,
) -> anyhow::Result<()> {
    let transaction = context.transaction();
    let placement = store
        .config
        .remotes
        .get(name)
        .ok_or_else(|| anyhow!("Unknown remote '{}'", name))?;
    let namespace = placement.namespace.clone();
    let mount = placement.mount.clone();
    let config = store.remote_config(name)?.clone();
    let mut fetch_args = vec![
        "fetch",
        "--prune",
        "--porcelain",
        &config.url,
        &config.ref_spec,
    ];
    if !announce {
        fetch_args.insert(1, "--quiet");
    }
    let mut command = transaction
        .git_command(&fetch_args, &[])?
        .with_stdout(Stdio::piped());
    if !announce {
        command = command.with_stderr(Stdio::null());
    }
    command
        .spawn()
        .with_context(|| format!("Failed to fetch remote '{}'", name))?;

    let mount_filter = josh_core::filter::to_filter(josh_core::filter::Op::Prefix(mount));
    if context.cache_policy().distributed()
        && let Err(error) =
            crate::commands::cache::fetch_remote_cache(transaction, &config.url, mount_filter)
    {
        eprintln!("Warning: could not fetch remote cache: {error}");
    }

    let output = Command::new("git")
        .args(["ls-remote", "--symref", &config.url, "HEAD"])
        .current_dir(store.git_dir())
        .output()
        .context("Failed to run git ls-remote")?;
    if !output.status.success() {
        bail!("Failed to determine default branch for remote '{}'", name);
    }
    let text = String::from_utf8(output.stdout)?;
    if let Some(branch) = parse_head_branch(&text) {
        store
            .config
            .remotes
            .get_mut(name)
            .expect("remote checked above")
            .head = Some(branch.to_owned());
        let head = namespace_prefix(&namespace, name) + "HEAD";
        transaction.create_symref(&head, &format!("refs/heads/{branch}"), "josh fetch")?;
    }
    if announce {
        eprintln!("Fetched from remote: {name}");
    }
    Ok(())
}
