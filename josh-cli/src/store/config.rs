use super::*;

pub(super) const STORE_VERSION: u32 = 4;
pub(super) const ROOT_PROJECTION: &str = "root";
pub(super) const SUPER_CONFIG_PATH: &str = "store";

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SuperWorktree {
    pub(super) projection: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SuperConfig {
    pub(super) version: u32,
    pub(super) remotes: BTreeMap<String, StoredRemote>,
    pub(super) projections: BTreeMap<String, Projection>,
    pub(super) worktrees: BTreeMap<String, SuperWorktree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projection {
    pub filter: String,
}

#[derive(Debug, Clone)]
pub struct RemotePlacement {
    pub namespace: String,
    pub mount: PathBuf,
    pub head: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorktreeRecord {
    pub path: PathBuf,
    pub projection: String,
}

#[derive(Debug, Clone)]
pub struct StoreConfig {
    pub projections: BTreeMap<String, Projection>,
    pub remotes: BTreeMap<String, RemotePlacement>,
    pub remote_configs: BTreeMap<String, RemoteConfig>,
    pub worktrees: BTreeMap<String, WorktreeRecord>,
}

impl Default for StoreConfig {
    fn default() -> Self {
        let mut projections = BTreeMap::new();
        projections.insert(
            ROOT_PROJECTION.to_owned(),
            Projection {
                filter: ":/".to_owned(),
            },
        );
        Self {
            projections,
            remotes: BTreeMap::new(),
            remote_configs: BTreeMap::new(),
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
        update_super_ref(&store, &transaction, &[])?;
        josh_changes::sync_change_refs_into_superhistory(&transaction)?;
        josh_changes::sync_link_refs_into_superhistory(&transaction)?;
        transaction.flush_mem_odb()?;
        Ok(store)
    }

    pub fn default_path() -> anyhow::Result<PathBuf> {
        dirs::data_local_dir()
            .map(|path| path.join("josh"))
            .context("Could not determine the user application-data directory; set JOSH_STORE or pass --store")
    }

    pub fn configured_path(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
        explicit
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("JOSH_STORE").map(PathBuf::from))
            .map(Ok)
            .unwrap_or_else(Self::default_path)
    }

    pub fn resolve(path: &Path, explicit: Option<&Path>, create: bool) -> anyhow::Result<Self> {
        if explicit.is_none()
            && let Some(store) = Self::discover_associated(path)?
        {
            return Ok(store);
        }

        let selected = Self::configured_path(explicit)?;
        if selected.join(".josh").exists() {
            return Self::load(selected);
        }
        if create {
            return Self::init(&selected);
        }
        bail!(
            "No josh store exists at {}; run 'josh clone' or 'josh remote add' first",
            selected.display()
        );
    }

    pub fn discover(path: &Path) -> anyhow::Result<Self> {
        Self::discover_associated(path)?.ok_or_else(|| anyhow!("Not in a josh store or worktree"))
    }

    fn discover_associated(path: &Path) -> anyhow::Result<Option<Self>> {
        for ancestor in path.ancestors() {
            if ancestor.join(".josh").join("HEAD").is_file() {
                return Self::load(ancestor.to_owned()).map(Some);
            }
        }
        let repo = match gix::discover(path) {
            Ok(repo) => repo,
            Err(_) => return Ok(None),
        };
        let Some(configured) = repo.config_snapshot().string("josh.store") else {
            return Ok(None);
        };
        let configured = std::str::from_utf8(configured.as_ref())
            .context("josh.store must contain a UTF-8 store path")?;
        Self::load(PathBuf::from(configured)).map(Some)
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
        let mut remotes = BTreeMap::new();
        let mut remote_configs = BTreeMap::new();
        for (name, remote) in stored.remotes {
            let config = remote.remote_config()?;
            remotes.insert(
                name.clone(),
                RemotePlacement {
                    namespace: remote.namespace,
                    mount: remote.mount,
                    head: remote.head,
                },
            );
            remote_configs.insert(name, config);
        }
        let sidecars = git_dir.join("josh").join("remotes");
        if sidecars.exists() {
            std::fs::remove_dir_all(sidecars)?;
        }
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
                    },
                )
            })
            .collect();
        let config = StoreConfig {
            projections: stored.projections,
            remotes,
            remote_configs,
            worktrees,
        };
        let store = Self { root, config };
        if !josh_changes::materialize_change_refs_from_superhistory(&transaction)? {
            josh_changes::sync_change_refs_into_superhistory(&transaction)?;
        }
        if !josh_changes::materialize_link_refs_from_superhistory(&transaction)? {
            josh_changes::sync_link_refs_into_superhistory(&transaction)?;
        }
        transaction.flush_mem_odb()?;
        Ok(store)
    }

    pub fn git_dir(&self) -> PathBuf {
        self.root.join(".josh")
    }

    pub fn remote_config(&self, name: &str) -> anyhow::Result<&RemoteConfig> {
        self.config
            .remote_configs
            .get(name)
            .with_context(|| format!("Unknown remote '{name}'"))
    }

    pub fn remote_name_at_mount(&self, path: &Path) -> anyhow::Result<Option<String>> {
        let (_, mount, name) = parse_mount_path(path)?;
        Ok(self
            .config
            .remotes
            .get(&name)
            .is_some_and(|placement| placement.mount == mount)
            .then_some(name))
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
        self.config
            .projections
            .insert(id.clone(), Projection { filter: canonical });
        Ok(id)
    }
}
pub(super) fn parse_mount_path(path: &Path) -> anyhow::Result<(String, PathBuf, String)> {
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
