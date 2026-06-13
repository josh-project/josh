use anyhow::{Context, anyhow};

/// Forge-specific behavior for a remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Forge {
    Github,
    Gerrit,
}

impl std::fmt::Display for Forge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Forge::Github => f.write_str("github"),
            Forge::Gerrit => f.write_str("gerrit"),
        }
    }
}

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

/// Resolved remote transport and filter configuration.
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

/// Use the common Git directory so linked worktrees share remote configuration.
fn remotes_dir(repo_path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("Failed to open repository at {}", repo_path.display()))?;

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
    let repo =
        gix::open(repo_path).context("Failed to open repository for legacy config migration")?;

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

    write_remote_config(
        repo_path,
        remote_name,
        &url,
        &filter_str,
        &fetch,
        None,
        None,
        None,
    )
    .context("Failed to migrate legacy config to new format")?;

    let filter_obj = josh_core::filter::parse(&filter_str)
        .with_context(|| format!("Failed to parse filter '{}'", filter_str))?;

    let filter_with_meta = filter_obj.with_meta("url", &url).with_meta("fetch", &fetch);

    log::info!(
        "Migrated remote '{}' from legacy git config to new file format",
        remote_name
    );

    Ok(RemoteConfig {
        url,
        ref_spec: fetch,
        filter_with_meta,
        forge: None,
        push_url: None,
        gerrit_mode: GerritMode::default(),
    })
}

/// Read a remote config, migrating legacy Git config when needed.
pub fn read_remote_config(
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

/// Persist remote configuration under the repository's common Git directory.
#[allow(clippy::too_many_arguments)]
pub fn write_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
    url: &str,
    filter: &str,
    fetch: &str,
    forge: Option<Forge>,
    push_url: Option<&str>,
    gerrit_mode: Option<GerritMode>,
) -> anyhow::Result<()> {
    let remotes_dir = remotes_dir(repo_path)?;

    std::fs::create_dir_all(&remotes_dir).with_context(|| {
        format!(
            "Failed to create remotes directory: {}",
            remotes_dir.display()
        )
    })?;

    let filter_obj = josh_core::filter::parse(filter)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;

    // Reject transport metadata instead of silently overwriting it.
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

    let content = josh_core::filter::as_file(filter_with_meta, 0);

    let remote_file = remotes_dir.join(format!("{}.josh", remote_name));
    std::fs::write(&remote_file, content).with_context(|| {
        format!(
            "Failed to write remote config file: {}",
            remote_file.display()
        )
    })?;

    Ok(())
}
