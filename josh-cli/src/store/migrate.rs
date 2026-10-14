use super::*;
fn ensure_clean_legacy_worktree(repository: &gix::Repository) -> anyhow::Result<()> {
    let status = repository
        .status(gix::progress::Discard)?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_submodules(None)
        .into_index_worktree_iter(Vec::new())?;
    for item in status {
        item?;
        bail!("Repository has uncommitted or untracked changes");
    }
    Ok(())
}

fn legacy_branches(
    repository: &gix::Repository,
) -> anyhow::Result<Vec<(String, gix_hash::ObjectId)>> {
    let mut branches = Vec::new();
    for reference in repository.references()?.local_branches()? {
        let reference = reference.map_err(|error| anyhow!("{error}"))?;
        let name = reference.name().shorten().to_str()?.to_owned();
        let oid = reference
            .target()
            .try_id()
            .map(ToOwned::to_owned)
            .with_context(|| format!("Local branch '{name}' is symbolic"))?;
        branches.push((name, oid));
    }
    Ok(branches)
}

fn remove_legacy_worktree_files(repository: &gix::Repository, root: &Path) -> anyhow::Result<()> {
    let index = repository.index()?;
    let mut directories = BTreeSet::new();
    for entry in index.entries() {
        let relative = gix::path::from_bstr(entry.path(&index)).into_owned();
        let path = root.join(&relative);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                std::fs::remove_dir_all(&path)?;
            }
            Ok(_) => std::fs::remove_file(&path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut parent = relative.parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            directories.insert(root.join(path));
            parent = path.parent();
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        match std::fs::remove_dir(&directory) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn convert_legacy_config_to_bare(
    repository: &gix::Repository,
    remote_names: &[String],
) -> anyhow::Result<()> {
    let path = repository.common_dir().join("config");
    let mut config =
        gix_config::File::from_path_no_includes(path.clone(), gix_config::Source::Local)?;
    config.set_raw_value("core.bare", "true")?;
    for name in remote_names {
        while config.remove_section("remote", name.as_str()).is_some() {}
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, config.to_bstring().as_slice())?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

pub fn migrate(
    path: &Path,
    worktree_name: &str,
    cache_policy: CachePolicy,
) -> anyhow::Result<PathBuf> {
    let repo = gix::discover(path).context("Not in a Git repository")?;
    let root = repo
        .workdir()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("Cannot migrate a bare repository"))?;
    if repo.config_snapshot().string("josh.store").is_some() {
        bail!("Repository is already a josh store");
    }
    ensure_clean_legacy_worktree(&repo)?;

    let remotes_dir = repo.common_dir().join("josh").join("remotes");
    let mut remote_names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&remotes_dir) {
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("josh")
                && let Some(name) = path.file_stem().and_then(|name| name.to_str())
            {
                remote_names.push(name.to_owned());
            }
        }
    }
    let snapshot = repo.config_snapshot();
    if let Some(sections) = snapshot.plumbing().sections_by_name("josh-remote") {
        for section in sections {
            if let Some(name) = section.header().subsection_name()
                && let Ok(name) = name.to_str()
                && !remote_names.iter().any(|candidate| candidate == name)
            {
                remote_names.push(name.to_owned());
            }
        }
    }
    remote_names.sort();
    if remote_names.is_empty() {
        bail!("No old-style josh remotes found");
    }
    let configs = remote_names
        .iter()
        .map(|name| read_legacy_remote_config(&root, name).map(|config| (name.clone(), config)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    if configs.len() != 1 {
        bail!("Migration requires exactly one old-style remote");
    }
    let remote_name = configs[0].0.clone();
    let old_filter = configs[0].1.semantic_filter();
    let filter = josh_core::filter::spec(
        josh_core::filter::to_filter(josh_core::filter::Op::Subdir(PathBuf::from(&remote_name)))
            .chain(old_filter),
    );

    let current_branch = repo
        .head_name()?
        .context("Cannot migrate a detached HEAD")?
        .shorten()
        .to_str()?
        .to_owned();
    let branches = legacy_branches(&repo)?;

    remove_legacy_worktree_files(&repo, &root)?;
    convert_legacy_config_to_bare(&repo, &remote_names)?;
    set_store_head(&repo)?;
    drop(repo);
    std::fs::rename(root.join(".git"), root.join(".josh"))
        .context("Failed to move legacy repository to .josh")?;
    let mut store = Store {
        root: dunce::canonicalize(&root)?,
        config: StoreConfig::default(),
    };
    for (name, config) in configs {
        add_remote(
            &mut store,
            &config.url,
            Path::new(&name),
            config.forge,
            config.push_url.as_deref(),
            Some(config.gerrit_mode),
        )?;
    }

    let git_dir = store.git_dir();
    let context = cache_policy.open(&git_dir, &git_dir, true, false)?;
    fetch(&mut store, &context, None)?;
    let transaction = context.transaction();

    let worktree_path = root.join(worktree_name);
    let worktree = add_worktree(
        &mut store,
        &worktree_path,
        &filter,
        Some(&current_branch),
        "HEAD",
        transaction,
    )?;
    for (name, oid) in &branches {
        let oid = oid.to_string();
        run_git(
            &worktree,
            ["update-ref", &format!("refs/heads/{name}"), &oid],
        )?;
    }
    let current_oid = branches
        .iter()
        .find_map(|(name, oid)| (name == &current_branch).then_some(oid))
        .ok_or_else(|| anyhow!("Current branch disappeared during migration"))?;
    run_git(
        &worktree,
        ["reset", "--hard", "--quiet", &current_oid.to_string()],
    )?;
    Ok(worktree)
}
