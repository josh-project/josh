use anyhow::{Context, anyhow};

pub use josh_view::Forge;

/// How `josh changes publish` maps a stack onto Gerrit changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum GerritMode {
    /// Publish only dependency-free changes as independent reviews.
    #[default]
    Independent,
    /// Push the whole commit history once as a single Gerrit relation chain.
    Stack,
}

impl std::fmt::Display for GerritMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GerritMode::Independent => f.write_str("independent"),
            GerritMode::Stack => f.write_str("stack"),
        }
    }
}

/// Meta keys that configure the remote itself rather than the filter semantics.
pub const TRANSPORT_META_KEYS: &[&str] = &["url", "fetch", "forge", "push", "gerrit-mode"];

#[derive(Debug, Clone)]
pub struct RemoteConfig {
    pub url: String,
    pub ref_spec: String,
    pub filter_with_meta: josh_core::filter::Filter,
    pub forge: Option<Forge>,
    /// Push destination for forks; `url` remains the fetch and review target.
    pub push_url: Option<String>,
    /// Stack mapping used only for Gerrit remotes.
    pub gerrit_mode: GerritMode,
}

impl RemoteConfig {
    /// Preserve history-affecting metadata while removing remote transport keys.
    pub fn semantic_filter(&self) -> josh_core::filter::Filter {
        self.filter_with_meta.without_meta_keys(TRANSPORT_META_KEYS)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredRemote {
    pub url: String,
    pub fetch: String,
    pub namespace: String,
    pub mount: std::path::PathBuf,
    pub forge: Option<String>,
    pub push_url: Option<String>,
    pub gerrit_mode: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
}

impl StoredRemote {
    pub fn remote_config(&self) -> anyhow::Result<RemoteConfig> {
        let forge = match self.forge.as_deref() {
            Some("github") => Some(Forge::Github),
            Some("gerrit") => Some(Forge::Gerrit),
            Some("test") => Some(Forge::Test),
            Some(forge) => return Err(anyhow!("Unsupported stored forge '{forge}'")),
            None => None,
        };
        let gerrit_mode = match self.gerrit_mode.as_deref() {
            Some("independent") => Some(GerritMode::Independent),
            Some("stack") => Some(GerritMode::Stack),
            Some(mode) => return Err(anyhow!("Unsupported stored Gerrit mode '{mode}'")),
            None => None,
        };
        let filter = josh_core::filter::spec(josh_core::filter::to_filter(
            josh_core::filter::Op::Subdir(self.mount.clone()),
        ));
        build_remote_config(
            &self.url,
            &filter,
            &self.fetch,
            forge,
            self.push_url.as_deref(),
            gerrit_mode,
        )
    }
}

fn open_repository(path: &std::path::Path) -> anyhow::Result<gix::Repository> {
    gix::open_opts(path, gix::open::Options::isolated().strict_config(true))
        .with_context(|| format!("Failed to open repository at {}", path.display()))
}

/// Resolve remote configuration through a linked repository to its josh store.
fn remotes_dir(repo_path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let repo = open_repository(repo_path)?;
    if let Some(store) = config_string(&repo, "josh.store")? {
        let store_git_dir = std::path::Path::new(&store).join(".josh");
        let store_repo = open_repository(&store_git_dir)
            .with_context(|| format!("Failed to open josh store at {store}"))?;
        return Ok(store_repo.common_dir().join("josh").join("remotes"));
    }
    Ok(repo.common_dir().join("josh").join("remotes"))
}

fn config_string(repo: &gix::Repository, key: &str) -> anyhow::Result<Option<String>> {
    repo.config_snapshot()
        .string(key)
        .map(|value| {
            std::str::from_utf8(value.as_ref())
                .map(ToOwned::to_owned)
                .map_err(Into::into)
        })
        .transpose()
}

pub fn migrate_legacy_config(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<RemoteConfig> {
    let repo = open_repository(repo_path)
        .context("Failed to open repository for legacy config migration")?;

    let url = match config_string(&repo, &format!("josh-remote.{}.url", remote_name))? {
        Some(url) => url,
        None => {
            let remote_file = remotes_dir(repo_path)?.join(format!("{}.josh", remote_name));

            return Err(anyhow!(
                "Remote '{}' not found in new format ({}) or legacy git config (josh-remote.{})",
                remote_name,
                remote_file.display(),
                remote_name
            ));
        }
    };

    let filter_str = config_string(&repo, &format!("josh-remote.{}.filter", remote_name))?
        .with_context(|| format!("Legacy config missing filter for remote '{}'", remote_name))?;

    let fetch = config_string(&repo, &format!("josh-remote.{}.fetch", remote_name))?
        .with_context(|| format!("Legacy config missing fetch for remote '{}'", remote_name))?;

    build_remote_config(&url, &filter_str, &fetch, None, None, None)
}

/// Read remote configuration from a clone that predates authoritative superhistory.
pub fn read_legacy_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<RemoteConfig> {
    let remotes_dir = remotes_dir(repo_path)?;
    let remote_file = remotes_dir.join(format!("{}.josh", remote_name));

    let content = match std::fs::read_to_string(&remote_file) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return migrate_legacy_config(repo_path, remote_name);
        }
        Err(e) => {
            return Err(anyhow!(
                "Failed to read remote config file: {}: {}",
                remote_file.display(),
                e
            ));
        }
    };

    let filter = josh_core::filter::parse(&content)
        .with_context(|| format!("Failed to parse filter from {}", remote_file.display()))?;

    let url = filter
        .get_meta("url")
        .ok_or_else(|| anyhow!("Missing 'url' metadata in remote config"))?;

    let fetch = filter
        .get_meta("fetch")
        .ok_or_else(|| anyhow!("Missing 'fetch' metadata in remote config"))?;

    let forge = filter
        .get_meta("forge")
        .map(|f| {
            use clap::ValueEnum;
            Forge::from_str(&f, true)
        })
        .transpose()
        .map_err(|f| anyhow!("Unknown forge: {f}"))?;

    let push_url = filter.get_meta("push");

    let gerrit_mode = filter
        .get_meta("gerrit-mode")
        .map(|m| {
            use clap::ValueEnum;
            GerritMode::from_str(&m, true)
        })
        .transpose()
        .map_err(|m| anyhow!("Unknown gerrit-mode: {m}"))?
        .unwrap_or_default();

    Ok(RemoteConfig {
        url,
        ref_spec: fetch,
        filter_with_meta: filter,
        forge,
        push_url,
        gerrit_mode,
    })
}

fn store_git_dir(repo_path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let repo = open_repository(repo_path)?;
    if let Some(store) = config_string(&repo, "josh.store")? {
        return Ok(std::path::Path::new(&store).join(".josh"));
    }
    Ok(repo.common_dir().to_owned())
}

/// Read authoritative remote configuration from `refs/josh/super`.
pub fn read_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<RemoteConfig> {
    let git_dir = store_git_dir(repo_path)?;
    let transaction = josh_core::cache::TransactionContext::new(
        &git_dir,
        std::sync::Arc::new(josh_core::cache::CacheStack::new()),
    )
    .open()?;
    let super_oid = transaction
        .resolve_ref(crate::refs::SUPER_REF)?
        .context("Josh store has no synthetic superhistory")?;
    let super_tree =
        josh_core::objects::CommitData::read(transaction.odb(), super_oid)?.tree_id()?;
    let path = std::path::Path::new("store")
        .join("remotes")
        .join(josh_git_serde::encode_key(remote_name));
    let remote_tree = josh_core::filter::tree::get_path_entry(
        &transaction,
        transaction.odb(),
        super_tree,
        &path,
    )?
    .filter(|entry| entry.mode.is_tree())
    .with_context(|| format!("Unknown remote '{remote_name}'"))?;
    let value = josh_git_serde::from_tree_oid(transaction.odb(), remote_tree.oid)?;
    let stored: StoredRemote = josh_git_serde::from_value(&value)?;
    stored.remote_config()
}

#[allow(clippy::too_many_arguments)]
pub fn build_remote_config(
    url: &str,
    filter: &str,
    fetch: &str,
    forge: Option<Forge>,
    push_url: Option<&str>,
    gerrit_mode: Option<GerritMode>,
) -> anyhow::Result<RemoteConfig> {
    let filter_obj = josh_core::filter::parse(filter)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;
    for key in TRANSPORT_META_KEYS {
        if filter_obj.get_meta(key).is_some() {
            return Err(anyhow!(
                "Filter must not set reserved meta key '{}': it is owned by the remote config",
                key
            ));
        }
    }
    let mut filter_with_meta = filter_obj.with_meta("url", url).with_meta("fetch", fetch);
    if let Some(forge) = forge {
        filter_with_meta = filter_with_meta.with_meta("forge", forge.to_string());
    }
    if let Some(push_url) = push_url {
        filter_with_meta = filter_with_meta.with_meta("push", push_url);
    }
    if let Some(gerrit_mode) = gerrit_mode {
        filter_with_meta = filter_with_meta.with_meta("gerrit-mode", gerrit_mode.to_string());
    }
    Ok(RemoteConfig {
        url: url.to_owned(),
        ref_spec: fetch.to_owned(),
        filter_with_meta,
        forge,
        push_url: push_url.map(ToOwned::to_owned),
        gerrit_mode: gerrit_mode.unwrap_or_default(),
    })
}
