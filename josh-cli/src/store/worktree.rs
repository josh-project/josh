use super::*;
pub(super) fn configure_worktree_remote(
    store: &Store,
    worktree: &Path,
    projection: &str,
    remote: &str,
) -> anyhow::Result<()> {
    let store_repo = store.git_dir();
    let store_url = url::Url::from_file_path(&store_repo)
        .map_err(|_| anyhow!("Failed to create store URL for {}", store_repo.display()))?
        .to_string();
    run_git(worktree, ["remote", "add", remote, &store_url])?;
    let namespace = format!("{projection}/{remote}");
    let upload = format!("josh serve-worktree --namespace {namespace} upload-pack");
    let receive = format!("josh serve-worktree --namespace {namespace} receive-pack");
    run_git(
        worktree,
        ["config", &format!("remote.{remote}.uploadpack"), &upload],
    )?;
    run_git(
        worktree,
        ["config", &format!("remote.{remote}.receivepack"), &receive],
    )?;
    Ok(())
}
fn set_worktree_remote_head(
    store: &Store,
    worktree: &Path,
    projection: &str,
    remote: &str,
) -> anyhow::Result<()> {
    let head_path = store
        .git_dir()
        .join(namespace_prefix(projection, remote))
        .join("HEAD");
    if let Ok(contents) = std::fs::read_to_string(head_path)
        && let Some(branch) = contents.trim().strip_prefix("ref: refs/heads/")
    {
        run_git(
            worktree,
            [
                "symbolic-ref",
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            ],
        )?;
    }
    Ok(())
}
fn copy_store_state_to_worktree(
    transaction: &josh_core::cache::Transaction,
    worktree: &Path,
) -> anyhow::Result<()> {
    for prefix in [josh_view::LINKS_REF_PREFIX, "refs/josh/submodules/"] {
        let mut desired = BTreeMap::new();
        transaction.for_each_ref_prefixed(prefix, |name, oid| {
            desired.insert(name.to_owned(), oid);
            Ok(())
        })?;
        let existing = ensure_success(
            git_output(worktree, ["for-each-ref", "--format=%(refname)", prefix])?,
            "Failed to list materialized store refs",
        )?;
        for name in existing.lines() {
            if !desired.contains_key(name) {
                run_git(worktree, ["update-ref", "-d", name])?;
            }
        }
        for (name, oid) in desired {
            run_git(worktree, ["update-ref", &name, &oid.to_string()])?;
        }
    }
    let mut desired = BTreeMap::new();
    for scope in josh_changes::all_changes_refs(transaction)? {
        let name = scope.ref_name();
        if let Some(oid) = transaction.resolve_ref(&name)? {
            desired.insert(name, oid);
        }
    }
    let existing = ensure_success(
        git_output(
            worktree,
            [
                "for-each-ref",
                "--format=%(refname)",
                "refs/josh/changes/",
                "refs/josh/remotes/",
            ],
        )?,
        "Failed to list materialized change refs",
    )?;
    for name in existing.lines().filter(|name| {
        name.starts_with("refs/josh/changes/")
            || name
                .strip_prefix("refs/josh/remotes/")
                .is_some_and(|name| name.contains("/changes/"))
    }) {
        if !desired.contains_key(name) {
            run_git(worktree, ["update-ref", "-d", name])?;
        }
    }
    for (name, oid) in desired {
        run_git(worktree, ["update-ref", &name, &oid.to_string()])?;
    }
    Ok(())
}

pub fn handle_link(
    store: &Store,
    args: &crate::commands::link::LinkArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    crate::commands::link::handle_link(args, transaction)?;
    josh_changes::sync_link_refs_into_superhistory(transaction)?;
    josh_changes::materialize_link_refs_from_superhistory(transaction)?;
    transaction.flush_mem_odb()?;
    for worktree in store.config.worktrees.values() {
        if worktree.path.exists() {
            copy_store_state_to_worktree(transaction, &worktree.path)?;
        }
    }
    Ok(())
}

pub fn add_worktree(
    store: &mut Store,
    path: &Path,
    filter: &str,
    branch: Option<&str>,
    requested_ref: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<PathBuf> {
    if store.config.remotes.is_empty() {
        bail!("Cannot add a worktree before adding a remote");
    }
    if path.exists() && path.read_dir()?.next().is_some() {
        bail!("Worktree path {} is not empty", path.display());
    }
    let projection = store.ensure_projection(filter)?;
    materialize_projections(store, transaction)?;
    josh_changes::sync_link_refs_into_superhistory(transaction)?;
    update_super_ref(store, transaction, &[])?;
    transaction.flush_mem_odb()?;
    std::fs::create_dir_all(path)?;
    run_git(path, ["init", "--quiet"])?;
    let absolute = dunce::canonicalize(path)?;
    let store_path = store
        .root
        .to_str()
        .context("Josh store path must be UTF-8")?;
    run_git(&absolute, ["config", "josh.store", store_path])?;
    run_git(&absolute, ["config", "josh.projection", &projection])?;

    let alternates = absolute
        .join(".git")
        .join("objects")
        .join("info")
        .join("alternates");
    let store_objects = store.git_dir().join("objects");
    std::fs::write(&alternates, format!("{}\n", store_objects.display()))?;

    for remote in store.config.remotes.keys() {
        configure_worktree_remote(store, &absolute, &projection, remote)?;
        let output = git_output(&absolute, ["fetch", "--quiet", remote])?;
        ensure_success(output, &format!("Failed to fetch store remote '{remote}'"))?;
        set_worktree_remote_head(store, &absolute, &projection, remote)?;
    }
    copy_store_state_to_worktree(transaction, &absolute)?;

    let (remote, source, default_branch) =
        select_checkout(store, &absolute, &projection, requested_ref)?;
    let local_branch = branch.unwrap_or(&default_branch);
    run_git(
        &absolute,
        ["checkout", "--quiet", "-b", local_branch, &source],
    )?;
    run_git(
        &absolute,
        [
            "branch",
            "--set-upstream-to",
            &format!("{remote}/{default_branch}"),
            local_branch,
        ],
    )?;

    let key = absolute.to_string_lossy().into_owned();
    store.config.worktrees.insert(
        key,
        WorktreeRecord {
            path: absolute.clone(),
            projection,
        },
    );
    update_super_ref(store, transaction, &[])?;
    transaction.flush_mem_odb()?;
    Ok(absolute)
}

fn select_checkout(
    store: &Store,
    worktree: &Path,
    projection: &str,
    requested_ref: &str,
) -> anyhow::Result<(String, String, String)> {
    if let Some(remote_ref) = requested_ref.strip_prefix("refs/remotes/") {
        let (remote, branch) = remote_ref
            .split_once('/')
            .ok_or_else(|| anyhow!("Invalid remote-tracking ref '{}'", requested_ref))?;
        if !store.config.remotes.contains_key(remote) {
            bail!("Unknown remote '{}' in ref '{}'", remote, requested_ref);
        }
        if !git_success(
            worktree,
            ["rev-parse", "--verify", "--quiet", requested_ref],
        )? {
            bail!(
                "Ref '{}' is not available at projection '{}'",
                requested_ref,
                projection
            );
        }
        let branch = if branch == "HEAD" {
            ensure_success(
                git_output(worktree, ["symbolic-ref", "--short", requested_ref])?,
                "Failed to resolve remote HEAD",
            )?
            .trim()
            .split_once('/')
            .map_or("HEAD", |(_, branch)| branch)
            .to_owned()
        } else {
            branch.to_owned()
        };
        return Ok((remote.to_owned(), requested_ref.to_owned(), branch));
    }

    let ordered = store.config.remotes.iter();
    for (remote, _) in ordered {
        let candidate = if requested_ref == "HEAD" {
            format!("refs/remotes/{remote}/HEAD")
        } else if let Some(branch) = requested_ref.strip_prefix("refs/heads/") {
            format!("refs/remotes/{remote}/{branch}")
        } else {
            format!("refs/remotes/{remote}/{requested_ref}")
        };
        if git_success(worktree, ["rev-parse", "--verify", "--quiet", &candidate])? {
            let branch = if requested_ref == "HEAD" {
                let output = git_output(worktree, ["symbolic-ref", "--short", &candidate])?;
                ensure_success(output, "Failed to resolve remote HEAD")?
                    .trim()
                    .rsplit('/')
                    .next()
                    .expect("symbolic ref has component")
                    .to_owned()
            } else if let Some(branch) = requested_ref.strip_prefix("refs/heads/") {
                branch.to_owned()
            } else {
                requested_ref.to_owned()
            };
            return Ok((remote.clone(), candidate, branch));
        }
    }
    bail!(
        "Ref '{}' is not available at projection '{}'",
        requested_ref,
        projection
    )
}

pub fn list_worktrees(store: &Store) {
    for record in store.config.worktrees.values() {
        let filter = &store.config.projections[&record.projection].filter;
        println!(
            "{}\t{}\t{}",
            record.path.display(),
            record.projection,
            filter
        );
    }
}

pub fn remove_worktree(
    store: &mut Store,
    path: &Path,
    force: bool,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let absolute = if path.exists() {
        dunce::canonicalize(path)?
    } else if path.is_absolute() {
        path.to_owned()
    } else {
        store.root.join(path)
    };
    let key = absolute.to_string_lossy().into_owned();
    let record = store
        .config
        .worktrees
        .get(&key)
        .ok_or_else(|| anyhow!("{} is not a registered worktree", absolute.display()))?;
    if record.path.exists() && !force {
        let status = git_output(&record.path, ["status", "--porcelain"])?;
        let status = ensure_success(status, "Failed to inspect worktree status")?;
        if !status.trim().is_empty() {
            bail!(
                "Worktree {} has uncommitted changes; use --force",
                record.path.display()
            );
        }
    }
    if record.path.exists() {
        std::fs::remove_dir_all(&record.path)
            .with_context(|| format!("Failed to remove {}", record.path.display()))?;
    }
    let removed = store
        .config
        .worktrees
        .remove(&key)
        .expect("registered worktree disappeared");
    if removed.projection != ROOT_PROJECTION
        && !store
            .config
            .worktrees
            .values()
            .any(|worktree| worktree.projection == removed.projection)
    {
        store.config.projections.remove(&removed.projection);
    }
    update_super_ref(store, transaction, &[])?;
    transaction.flush_mem_odb()
}

pub(super) fn fetch_worktrees(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    for worktree in store.config.worktrees.values() {
        if !worktree.path.exists() {
            continue;
        }
        let output = git_output(&worktree.path, ["fetch", "--all", "--prune", "--quiet"])?;
        ensure_success(
            output,
            &format!("Failed to update worktree {}", worktree.path.display()),
        )?;
        for remote in store.config.remotes.keys() {
            set_worktree_remote_head(store, &worktree.path, &worktree.projection, remote)?;
        }
        copy_store_state_to_worktree(transaction, &worktree.path)?;
    }
    Ok(())
}

pub fn current_projection(path: &Path) -> anyhow::Result<String> {
    let repo = gix::discover(path)?;
    repo.config_snapshot()
        .string("josh.projection")
        .map(|value| String::from_utf8_lossy(value.as_ref()).into_owned())
        .ok_or_else(|| anyhow!("Command must be run in a josh worktree"))
}

pub fn validate_worktree_service(repository: &Path, namespace: &str) -> anyhow::Result<PathBuf> {
    let mut components = namespace.split('/');
    let projection = components.next().unwrap_or_default();
    let remote = components.next().unwrap_or_default();
    if projection.is_empty()
        || remote.is_empty()
        || components.next().is_some()
        || [projection, remote].iter().any(|component| {
            *component == "." || *component == ".." || component.chars().any(char::is_control)
        })
    {
        bail!("Invalid worktree namespace '{}'", namespace);
    }

    let root = if repository.file_name().is_some_and(|name| name == ".josh") {
        repository
            .parent()
            .ok_or_else(|| anyhow!("Josh repository has no store directory"))?
    } else {
        repository
    };
    let store = Store::load(root.to_owned()).with_context(|| {
        format!(
            "Worktree service repository '{}' is not a josh store",
            repository.display()
        )
    })?;
    if !store.config.projections.contains_key(projection)
        || !store.config.remotes.contains_key(remote)
    {
        bail!("Unknown worktree namespace '{}'", namespace);
    }
    Ok(store.git_dir())
}

pub fn namespace_prefix(projection: &str, remote: &str) -> String {
    format!("refs/namespaces/{projection}/refs/namespaces/{remote}/")
}

pub fn namespace_heads_prefix(projection: &str, remote: &str) -> String {
    namespace_prefix(projection, remote) + "refs/heads/"
}
