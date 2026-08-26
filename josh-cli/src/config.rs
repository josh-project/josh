use anyhow::{Context, anyhow};

use crate::forge::{Forge, GerritMode};

/// Meta keys that configure the remote itself rather than the filter semantics.
pub const TRANSPORT_META_KEYS: &[&str] = &["url", "fetch", "forge", "push", "gerrit-mode"];

pub struct RemoteConfig {
    pub url: String,
    pub ref_spec: String,
    pub filter_with_meta: josh_core::filter::Filter,
    pub forge: Option<Forge>,
    /// Optional separate push destination (a fork). When set, branches are
    /// pushed here while `url` remains the fetch source and PR target. Analogous
    /// to git's `remote.<name>.pushurl`.
    pub push_url: Option<String>,
    /// How `josh changes publish` maps a stack onto Gerrit changes. Only
    /// meaningful when `forge` is `Gerrit`; defaults to `Independent`.
    pub gerrit_mode: GerritMode,
}

impl RemoteConfig {
    /// The filter to apply: transport keys (`url`, `fetch`, `forge`) stripped,
    /// semantic meta args (`history`, `gpgsig`, ...) retained. Semantic args
    /// change the filtered history, so dropping them (via `peel()`) would
    /// produce SHAs that diverge from what josh-proxy/josh-filter compute for
    /// the same filter spec.
    pub fn semantic_filter(&self) -> josh_core::filter::Filter {
        self.filter_with_meta.without_meta_keys(TRANSPORT_META_KEYS)
    }
}

/// Resolve the directory holding josh remote config files.
///
/// The config lives in the repository's *common* git directory so it is shared
/// across linked worktrees. Reconstructing `<workdir>/.git` would be wrong when
/// invoked from a worktree, where the gitdir is `<main>/.git/worktrees/<name>`
/// but the shared config still lives under `<main>/.git`.
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
    // File doesn't exist, try legacy git config
    let repo =
        gix::open(repo_path).context("Failed to open repository for legacy config migration")?;

    // Try to read from legacy josh-remote config
    let url_key = format!("josh-remote.{}.url", remote_name);
    let url = match config_string(&repo, &url_key)? {
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

    // Migrate to new format by writing the file
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

    // Parse the filter to return
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

/// Read remote configuration from .git/josh/remotes/<name>.josh file
/// Falls back to legacy git config josh-remote section if file doesn't exist
pub fn read_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<RemoteConfig> {
    let remotes_dir = remotes_dir(repo_path)?;
    let remote_file = remotes_dir.join(format!("{}.josh", remote_name));

    // Try to read from the new file format first
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

    // Parse the filter from the file
    let filter = josh_core::filter::parse(&content)
        .with_context(|| format!("Failed to parse filter from {}", remote_file.display()))?;

    // Extract metadata
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

/// Write remote configuration to .git/josh/remotes/<name>.josh file
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

    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&remotes_dir).with_context(|| {
        format!(
            "Failed to create remotes directory: {}",
            remotes_dir.display()
        )
    })?;

    // Parse the filter
    let filter_obj = josh_core::filter::parse(filter)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;

    // The transport keys are owned by the remote config; a user filter that
    // sets one of them would be silently overwritten below.
    for key in TRANSPORT_META_KEYS {
        if filter_obj.get_meta(key).is_some() {
            return Err(anyhow!(
                "Filter must not set reserved meta key '{}': it is owned by the remote config",
                key
            ));
        }
    }

    // Wrap the filter with metadata
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

    // Serialize the filter with metadata
    let content = josh_core::filter::as_file(filter_with_meta, 0);

    // Write to file
    let remote_file = remotes_dir.join(format!("{}.josh", remote_name));
    std::fs::write(&remote_file, content).with_context(|| {
        format!(
            "Failed to write remote config file: {}",
            remote_file.display()
        )
    })?;

    Ok(())
}
