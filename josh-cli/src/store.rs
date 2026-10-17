use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use gix::bstr::ByteSlice;

use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::config::{read_remote_config, write_remote_config};
use crate::forge::{Forge, GerritMode};

const STORE_VERSION: u32 = 4;
const ROOT_PROJECTION: &str = "root";
const SUPER_REF: &str = "refs/josh/super";
const SUPER_CONFIG_PATH: &str = "store";

#[derive(Debug, Serialize, Deserialize)]
struct SuperRemote {
    url: String,
    fetch: String,
    namespace: String,
    mount: PathBuf,
    forge: Option<String>,
    push_url: Option<String>,
    gerrit_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SuperWorktree {
    projection: String,
    filter: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SuperConfig {
    version: u32,
    remotes: BTreeMap<String, SuperRemote>,
    projections: BTreeMap<String, Projection>,
    worktrees: BTreeMap<String, SuperWorktree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projection {
    pub filter: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePlacement {
    pub namespace: String,
    pub mount: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRecord {
    pub path: PathBuf,
    pub projection: String,
    pub filter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    pub version: u32,
    pub projections: BTreeMap<String, Projection>,
    pub remotes: BTreeMap<String, RemotePlacement>,
    pub worktrees: BTreeMap<String, WorktreeRecord>,
}

impl Default for StoreConfig {
    fn default() -> Self {
        let mut projections = BTreeMap::new();
        projections.insert(
            ROOT_PROJECTION.to_owned(),
            Projection {
                filter: ":/".to_owned(),
                parent: None,
            },
        );
        Self {
            version: STORE_VERSION,
            projections,
            remotes: BTreeMap::new(),
            worktrees: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
    pub config: StoreConfig,
}

impl Store {
    pub fn init(path: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create store directory {}", path.display()))?;
        if path.join(".josh").exists() || path.join(".git").exists() {
            bail!("{} already contains a Git repository", path.display());
        }
        let repository = gix::init_bare(path.join(".josh"))
            .context("Failed to initialize bare store repository")?;

        let root = dunce::canonicalize(path)
            .with_context(|| format!("Failed to resolve store path {}", path.display()))?;
        let store = Self {
            root,
            config: StoreConfig::default(),
        };
        let git_dir = store.git_dir();
        set_store_head(&repository)?;
        let transaction = josh_core::cache::TransactionContext::new(
            &git_dir,
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        )
        .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE)
        .open()?;
        update_super_ref(&store, &transaction)?;
        transaction.flush_mem_odb()?;
        Ok(store)
    }

    pub fn discover(path: &Path) -> anyhow::Result<Self> {
        if path.join(".josh").join("HEAD").is_file() {
            return Self::load(path.to_owned());
        }
        let repo = gix::discover(path).context("Not in a josh store or worktree")?;
        let configured = repo
            .config_snapshot()
            .string("josh.store")
            .map(|value| String::from_utf8_lossy(value.as_ref()).into_owned())
            .ok_or_else(|| anyhow!("Not in a josh store or worktree"))?;
        if configured == "true" {
            let common_dir = repo.common_dir();
            let root = common_dir
                .file_name()
                .is_some_and(|name| name == ".josh")
                .then(|| common_dir.parent())
                .flatten()
                .map(Path::to_path_buf)
                .ok_or_else(|| anyhow!("Bare josh store is not inside a store directory"))?;
            Self::load(root)
        } else {
            Self::load(PathBuf::from(configured))
        }
    }

    pub fn load(path: PathBuf) -> anyhow::Result<Self> {
        let root = dunce::canonicalize(&path)
            .with_context(|| format!("Failed to resolve store path {}", path.display()))?;
        let git_dir = root.join(".josh");
        let transaction = josh_core::cache::TransactionContext::new(
            &git_dir,
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        )
        .open()?;
        let super_oid = transaction
            .resolve_ref(SUPER_REF)?
            .ok_or_else(|| anyhow!("Josh store has no synthetic superhistory"))?;
        let super_tree =
            josh_core::objects::CommitData::read(transaction.odb(), super_oid)?.tree_id()?;
        let config_tree = josh_core::filter::tree::get_path_entry(
            &transaction,
            transaction.odb(),
            super_tree,
            Path::new(SUPER_CONFIG_PATH),
        )?
        .filter(|entry| entry.mode.is_tree())
        .ok_or_else(|| anyhow!("Josh superhistory has no store registry"))?
        .oid;
        let value = josh_git_serde::from_tree_oid(transaction.odb(), config_tree)?;
        let stored: SuperConfig = josh_git_serde::from_value(&value)?;
        if stored.version != STORE_VERSION {
            bail!(
                "Unsupported store version {} (expected {})",
                stored.version,
                STORE_VERSION
            );
        }
        let remotes = stored
            .remotes
            .into_iter()
            .map(|(name, remote)| {
                (
                    name,
                    RemotePlacement {
                        namespace: remote.namespace,
                        mount: remote.mount,
                    },
                )
            })
            .collect();
        let worktrees = stored
            .worktrees
            .into_iter()
            .map(|(name, worktree)| {
                let path = PathBuf::from(name);
                let path = if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                };
                let key = path.to_string_lossy().into_owned();
                (
                    key,
                    WorktreeRecord {
                        path,
                        projection: worktree.projection,
                        filter: worktree.filter,
                    },
                )
            })
            .collect();
        let config = StoreConfig {
            version: stored.version,
            projections: stored.projections,
            remotes,
            worktrees,
        };
        Ok(Self { root, config })
    }

    pub fn git_dir(&self) -> PathBuf {
        self.root.join(".josh")
    }

    pub fn ensure_projection(&mut self, filter: &str) -> anyhow::Result<String> {
        let parsed = josh_core::filter::parse(filter)
            .with_context(|| format!("Failed to parse filter '{}'", filter))?;
        for step in josh_core::filter::flatten_chain(parsed) {
            for key in crate::config::TRANSPORT_META_KEYS {
                if step.get_meta(key).is_some() {
                    bail!(
                        "Filter must not set reserved meta key '{}': it is owned by remote configuration",
                        key
                    );
                }
            }
        }
        let canonical = josh_core::filter::spec(parsed);
        if let Some(projection) = self
            .config
            .projections
            .iter()
            .find_map(|(id, projection)| (projection.filter == canonical).then(|| id.clone()))
        {
            return Ok(projection);
        }
        if canonical == ":/" {
            return Ok(ROOT_PROJECTION.to_owned());
        }
        let id = parsed.id().to_string();
        self.config.projections.insert(
            id.clone(),
            Projection {
                filter: canonical,
                parent: Some(ROOT_PROJECTION.to_owned()),
            },
        );
        Ok(id)
    }
}
fn parse_mount_path(path: &Path) -> anyhow::Result<(String, PathBuf, String)> {
    if path.to_str().is_some_and(|path| path.starts_with(':')) {
        bail!("Remote mounts take plain paths, not filter expressions");
    }
    let components = path
        .components()
        .map(|component| match component {
            std::path::Component::Normal(component) => component
                .to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| anyhow!("Remote mount paths must be UTF-8")),
            _ => Err(anyhow!(
                "Remote mounts must be non-root normalized relative paths"
            )),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if components.is_empty() {
        bail!("Remote mounts must be non-root normalized relative paths");
    }
    let normalized = components.join("/");
    let mount = PathBuf::from(&normalized);
    let filter = josh_core::filter::spec(josh_core::filter::to_filter(
        josh_core::filter::Op::Subdir(mount.clone()),
    ));
    let name = josh_git_serde::encode_key(&normalized);
    Ok((filter, mount, name))
}

fn set_store_head(repository: &gix::Repository) -> anyhow::Result<()> {
    repository.edit_reference(gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: Default::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: gix::refs::Target::Symbolic(SUPER_REF.try_into()?),
        },
        name: "HEAD".try_into()?,
        deref: false,
    })?;
    Ok(())
}

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
    distributed_cache: bool,
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
        .map(|name| read_remote_config(&root, name).map(|config| (name.clone(), config)))
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
    let mut cache_stack = josh_core::cache::CacheStack::new()
        .with_backend(josh_core::cache::SledCacheBackend::new(&git_dir));
    if distributed_cache {
        cache_stack =
            cache_stack.with_backend(josh_core::cache::DistributedCacheBackend::new(&git_dir)?);
    }
    let transaction =
        josh_core::cache::TransactionContext::new(&git_dir, std::sync::Arc::new(cache_stack))
            .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE)
            .open()?;
    fetch(&store, &transaction, None, distributed_cache)?;

    let worktree_path = root.join(worktree_name);
    let worktree = add_worktree(
        &mut store,
        &worktree_path,
        &filter,
        Some(&current_branch),
        "HEAD",
        &transaction,
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
    write_remote_config(
        &store.git_dir(),
        &name,
        url,
        &filter,
        &refspec,
        forge,
        push_url,
        gerrit_mode,
    )?;
    store
        .config
        .remotes
        .insert(name.clone(), RemotePlacement { namespace, mount });
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

    let config = store
        .git_dir()
        .join("josh")
        .join("remotes")
        .join(format!("{name}.josh"));
    if config.exists() {
        std::fs::remove_file(&config)
            .with_context(|| format!("Failed to remove {}", config.display()))?;
    }
    update_super_ref(store, transaction)?;
    transaction.flush_mem_odb()
}

pub fn fetch(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
    selected_remote: Option<&str>,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    fetch_impl(store, transaction, selected_remote, distributed_cache, true)
}

pub fn fetch_quiet(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
    selected_remote: Option<&str>,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    fetch_impl(
        store,
        transaction,
        selected_remote,
        distributed_cache,
        false,
    )
}

fn fetch_impl(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
    selected_remote: Option<&str>,
    distributed_cache: bool,
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
        fetch_upstream(store, transaction, name, distributed_cache, announce)?;
    }
    materialize_projections(store, transaction)?;
    update_super_ref(store, transaction)?;
    transaction.flush_mem_odb()?;
    fetch_worktrees(store)?;
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
    crate::commands::push::handle_store_push(
        args,
        &relay.remote,
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
    for refname in refs {
        transaction.delete_ref(&refname, josh_core::cache::Expected::Any)?;
    }
    Ok(())
}

pub fn sync_worktree_change_metadata(store: &Store, distributed_cache: bool) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    let (remote, _) = select_worktree_remote(store, None, &worktree)?;
    let git_dir = store.git_dir();
    let mut cache_stack = josh_core::cache::CacheStack::new()
        .with_backend(josh_core::cache::SledCacheBackend::new(&git_dir));
    if distributed_cache {
        cache_stack =
            cache_stack.with_backend(josh_core::cache::DistributedCacheBackend::new(&git_dir)?);
    }
    let transaction =
        josh_core::cache::TransactionContext::new(&git_dir, std::sync::Arc::new(cache_stack))
            .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE)
            .open()?;
    import_worktree_refs(&remote, &["refs/josh/changes/"], &transaction)?;
    let result = josh_changes::merge_canonical_metadata(&transaction);
    clear_imported_change_refs(&transaction)?;
    result?;
    transaction.flush_mem_odb()
}

fn native_remote_head(
    store: &Store,
    remote: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<(String, gix_hash::ObjectId)> {
    let placement = store
        .config
        .remotes
        .get(remote)
        .with_context(|| format!("Unknown remote '{}'", remote))?;
    let head_ref = format!("refs/remotes/{remote}/HEAD");
    let target = transaction
        .symref_target(&head_ref)?
        .with_context(|| format!("Remote '{}' has no default branch", remote))?;
    let branch = target
        .strip_prefix(&format!("refs/remotes/{remote}/"))
        .with_context(|| format!("Invalid remote HEAD target '{}'", target))?
        .to_owned();
    let oid = transaction
        .resolve_ref(&(namespace_heads_prefix(&placement.namespace, remote) + &branch))?
        .with_context(|| format!("Remote '{}' default branch was not fetched", remote))?;
    Ok((branch, oid))
}

pub fn discover_submodule_remotes(
    store: &Store,
    remote: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<Vec<crate::commands::link::SubmoduleRemote>> {
    let config = read_remote_config(&store.git_dir(), remote)?;
    let (_, commit) = native_remote_head(store, remote, transaction)?;
    crate::commands::link::submodule_remotes(transaction, commit, &config.url)
}

pub fn derive_submodule_filter(
    store: &Store,
    remote: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<String> {
    let config = read_remote_config(&store.git_dir(), remote)?;
    let placement = store
        .config
        .remotes
        .get(remote)
        .with_context(|| format!("Unknown remote '{}'", remote))?;
    let (branch, commit) = native_remote_head(store, remote, transaction)?;
    let inline_filter = crate::commands::link::add_submodule_links_at_commit(
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
        &["refs/josh/links/", "refs/josh/changes/"],
        transaction,
    )?;
    let worktree = std::env::current_dir()?;
    let author = ensure_success(
        git_output(&worktree, ["config", "--get", "user.email"])?,
        "user.email is required for publishing changes",
    )?;
    let result = crate::commands::push::handle_store_publish(
        args,
        &relay.remote,
        &relay.refspecs,
        relay.filter,
        author.trim(),
        transaction,
    );
    clear_imported_change_refs(transaction)?;
    result
}
pub fn pull(
    store: &Store,
    args: &crate::commands::pull::PullArgs,
    transaction: &josh_core::cache::Transaction,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    current_projection(&worktree)?;
    let (remote, _) = select_worktree_remote(store, args.remote.as_deref(), &worktree)?;
    fetch(store, transaction, Some(&remote), distributed_cache)?;
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
    store: &Store,
    args: &crate::commands::pull::PullArgs,
    transaction: &josh_core::cache::Transaction,
    distributed_cache: bool,
) -> anyhow::Result<()> {
    let worktree = std::env::current_dir()?;
    let (remote, _) = select_worktree_remote(store, args.remote.as_deref(), &worktree)?;
    fetch(store, transaction, Some(&remote), distributed_cache)?;
    let paths = josh_core::git::discover_repository_paths()?;
    let mut cache_stack = josh_core::cache::CacheStack::new()
        .with_backend(josh_core::cache::SledCacheBackend::new(&paths.common_dir));
    if distributed_cache {
        cache_stack = cache_stack.with_backend(josh_core::cache::DistributedCacheBackend::new(
            &paths.common_dir,
        )?);
    }
    let worktree_transaction = josh_core::cache::TransactionContext::new(
        &paths.workdir_or_gitdir,
        std::sync::Arc::new(cache_stack),
    )
    .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE)
    .open()?;
    crate::commands::pull::integrate_fetched(&remote, &worktree_transaction)
}

fn fetch_upstream(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
    name: &str,
    distributed_cache: bool,
    announce: bool,
) -> anyhow::Result<()> {
    let placement = store
        .config
        .remotes
        .get(name)
        .ok_or_else(|| anyhow!("Unknown remote '{}'", name))?;
    let config = read_remote_config(&store.git_dir(), name)?;
    transaction
        .git_command(
            &[
                "fetch",
                "--prune",
                "--porcelain",
                &config.url,
                &config.ref_spec,
            ],
            &[],
        )?
        .with_stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to fetch remote '{}'", name))?;

    let mount_filter =
        josh_core::filter::to_filter(josh_core::filter::Op::Prefix(placement.mount.clone()));
    if distributed_cache
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
        let head = namespace_prefix(&placement.namespace, name) + "HEAD";
        transaction.create_symref(&head, &format!("refs/heads/{branch}"), "josh fetch")?;
        transaction.create_symref(
            &format!("refs/remotes/{name}/HEAD"),
            &format!("refs/remotes/{name}/{branch}"),
            "josh fetch",
        )?;
    }
    if announce {
        eprintln!("Fetched from remote: {name}");
    }
    Ok(())
}

pub fn materialize_projections(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    for (remote_name, placement) in &store.config.remotes {
        let native_refs = refs_in_namespace(transaction, &placement.namespace, remote_name)?;
        let native_branches = native_refs
            .iter()
            .map(|(branch, _)| branch.clone())
            .collect();
        let backing_prefix = backing_heads_prefix(remote_name);
        reconcile_refs(transaction, &backing_prefix, &native_branches)?;
        for (branch, oid) in &native_refs {
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
            josh_core::filter_refs(transaction, mount_filter, &native_refs);
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
        copy_namespace_head(
            transaction,
            &placement.namespace,
            ROOT_PROJECTION,
            remote_name,
        )?;

        for (target_id, target) in &store.config.projections {
            if target_id == ROOT_PROJECTION || target.parent.as_deref() != Some(ROOT_PROJECTION) {
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
                transaction.update_ref(
                    &format!("refs/josh/filtered/{filtered_prefix}/heads/{branch}"),
                    josh_core::cache::Expected::Any,
                    oid,
                    "josh filter",
                )?;
            }
            copy_namespace_head(transaction, ROOT_PROJECTION, target_id, remote_name)?;
        }
    }
    Ok(())
}
fn update_super_ref(
    store: &Store,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let odb = transaction.odb();
    let mut remotes = BTreeMap::new();
    for (name, placement) in &store.config.remotes {
        let config = read_remote_config(&store.git_dir(), name)?;
        remotes.insert(
            name.clone(),
            SuperRemote {
                url: config.url,
                fetch: config.ref_spec,
                namespace: placement.namespace.clone(),
                mount: placement.mount.clone(),
                forge: config.forge.map(|forge| forge.to_string()),
                push_url: config.push_url,
                gerrit_mode: config.gerrit_mode.to_string(),
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
                    filter: record.filter.clone(),
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
    let mut tree = josh_core::filter::tree::insert_oid(
        odb,
        josh_core::filter::tree::empty_id(),
        Path::new(SUPER_CONFIG_PATH),
        config_tree,
        0o040000,
    )?;
    if let Some(previous) = previous {
        let previous_tree = josh_core::objects::CommitData::read(odb, previous)?.tree_id()?;
        if let Some(changes) = josh_core::filter::tree::get_path_entry(
            transaction,
            odb,
            previous_tree,
            Path::new("changes"),
        )? {
            tree = josh_core::filter::tree::insert_oid(
                odb,
                tree,
                Path::new("changes"),
                changes.oid,
                changes.mode.value() as i32,
            )?;
        }
    }

    for (name, placement) in &store.config.remotes {
        for (branch, oid) in refs_in_namespace(transaction, &placement.namespace, name)? {
            tree = josh_core::filter::tree::insert_oid(
                odb,
                tree,
                &Path::new("remotes")
                    .join(name)
                    .join("refs")
                    .join("heads")
                    .join(branch),
                oid,
                0o160000,
            )?;
        }
        let head_ref = namespace_prefix(&placement.namespace, name) + "HEAD";
        if let Some(target) = transaction.symref_target(&head_ref)? {
            let oid = transaction
                .resolve_ref(&(namespace_prefix(&placement.namespace, name) + &target))?
                .with_context(|| format!("Remote '{}' HEAD target is missing", name))?;
            tree = josh_core::filter::tree::insert_oid(
                odb,
                tree,
                &Path::new("remotes").join(name).join("HEAD"),
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

fn copy_namespace_head(
    transaction: &josh_core::cache::Transaction,
    source_projection: &str,
    target_projection: &str,
    remote: &str,
) -> anyhow::Result<()> {
    let source = namespace_prefix(source_projection, remote) + "HEAD";
    if let Some(target) = transaction.symref_target(&source)? {
        transaction.create_symref(
            &(namespace_prefix(target_projection, remote) + "HEAD"),
            &target,
            "josh filter",
        )?;
    }
    Ok(())
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
    format!("refs/josh/remotes/{remote}/heads/")
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
fn configure_worktree_remote(
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
    for prefix in ["refs/josh/links/", "refs/josh/submodules/"] {
        let mut refs = Vec::new();
        transaction.for_each_ref_prefixed(prefix, |name, oid| {
            refs.push((name.to_owned(), oid));
            Ok(())
        })?;
        for (name, oid) in refs {
            run_git(worktree, ["update-ref", &name, &oid.to_string()])?;
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
    let filter = store.config.projections[&projection].filter.clone();
    materialize_projections(store, transaction)?;
    update_super_ref(store, transaction)?;
    transaction.flush_mem_odb()?;
    std::fs::create_dir_all(path)?;
    run_git(path, ["init", "--quiet"])?;
    let absolute = dunce::canonicalize(path)?;
    let store_path = store.root.to_string_lossy().into_owned();
    run_git(&absolute, ["config", "josh.store", &store_path])?;
    run_git(&absolute, ["config", "josh.projection", &projection])?;
    run_git(&absolute, ["config", "josh.filter", &filter])?;

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
            filter,
        },
    );
    update_super_ref(store, transaction)?;
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
        println!(
            "{}\t{}\t{}",
            record.path.display(),
            record.projection,
            record.filter
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
    update_super_ref(store, transaction)?;
    transaction.flush_mem_odb()
}

fn fetch_worktrees(store: &Store) -> anyhow::Result<()> {
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

fn parse_head_branch(output: &str) -> Option<&str> {
    output
        .lines()
        .next()?
        .split('\t')
        .next()?
        .strip_prefix("ref: refs/heads/")
}

fn check_remote_name(name: &str) -> anyhow::Result<()> {
    if name.contains('/') {
        bail!("Remote names must not contain '/'");
    }
    gix::validate::reference::name_partial(name.as_bytes().into())
        .with_context(|| format!("Invalid remote name '{name}'"))?;
    Ok(())
}

fn git_success<const N: usize>(cwd: &Path, args: [&str; N]) -> anyhow::Result<bool> {
    Ok(Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> anyhow::Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("Failed to run git")
}

fn ensure_success(output: Output, context: &str) -> anyhow::Result<String> {
    if !output.status.success() {
        bail!(
            "{}: {}",
            context,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("Git output was not UTF-8")
}

fn run_git<I, S>(cwd: &Path, args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("Failed to run git")?;
    ensure_success(output, "Git command failed")?;
    Ok(())
}
