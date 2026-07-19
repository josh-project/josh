use std::io::Write;

use anyhow::{Context, anyhow};

use crate::forge::Forge;

pub struct RemoteConfig {
    pub url: String,
    pub ref_spec: String,
    pub filter_with_meta: josh_core::filter::Filter,
    pub forge: Option<Forge>,
}

/// Resolve the directory holding josh remote config files.
///
/// The config lives in the repository's *common* git directory so it is shared
/// across linked worktrees. Reconstructing `<workdir>/.git` would be wrong when
/// invoked from a worktree, where the gitdir is `<main>/.git/worktrees/<name>`
/// but the shared config still lives under `<main>/.git`.
fn remotes_dir(repo_path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let repo = git2::Repository::open(repo_path)
        .with_context(|| format!("Failed to open repository at {}", repo_path.display()))?;

    Ok(repo.commondir().join("josh").join("remotes"))
}

pub fn remote_config_path(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<std::path::PathBuf> {
    Ok(remotes_dir(repo_path)?.join(format!("{remote_name}.josh")))
}

pub fn list_remote_names(repo_path: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let directory = remotes_dir(repo_path)?;
    let mut names = Vec::new();
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    if let Some(entries) = entries {
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("josh") {
                continue;
            }
            if let Some(name) = path.file_stem().and_then(|value| value.to_str()) {
                names.push(name.to_string());
            }
        }
    }
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;
    let config = repo.config().context("Failed to read Git configuration")?;
    if let Ok(entries) = config.entries(Some("josh-remote.*.url")) {
        entries.for_each(|entry| {
            if let Some(name) = entry
                .name()
                .and_then(|name| name.strip_prefix("josh-remote."))
                .and_then(|name| name.strip_suffix(".url"))
            {
                names.push(name.to_string());
            }
        })?;
    }

    names.sort();
    names.dedup();
    Ok(names)
}

pub fn remove_remote_config(repo_path: &std::path::Path, remote_name: &str) -> anyhow::Result<()> {
    let path = remote_config_path(repo_path, remote_name)?;
    std::fs::remove_file(&path)
        .with_context(|| format!("Failed to remove remote config '{}'", path.display()))?;

    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;
    let mut config = repo.config().context("Failed to read Git configuration")?;
    for key in ["url", "filter", "fetch"] {
        let _ = config.remove(&format!("josh-remote.{remote_name}.{key}"));
    }
    Ok(())
}

pub fn migrate_legacy_config(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<RemoteConfig> {
    // File doesn't exist, try legacy git config
    let repo = git2::Repository::open(repo_path)
        .context("Failed to open repository for legacy config migration")?;

    let config = repo.config().context("Failed to get git config")?;

    // Try to read from legacy josh-remote config
    let url = match config.get_string(&format!("josh-remote.{}.url", remote_name)) {
        Ok(url) => url,
        Err(_) => {
            let remote_file = remotes_dir(repo_path)?.join(format!("{}.josh", remote_name));

            return Err(anyhow!(
                "Remote '{}' not found in new format ({}) or legacy git config (josh-remote.{})",
                remote_name,
                remote_file.display(),
                remote_name
            ));
        }
    };

    let filter_str = config
        .get_string(&format!("josh-remote.{}.filter", remote_name))
        .with_context(|| format!("Legacy config missing filter for remote '{}'", remote_name))?;

    let fetch = config
        .get_string(&format!("josh-remote.{}.fetch", remote_name))
        .with_context(|| format!("Legacy config missing fetch for remote '{}'", remote_name))?;

    // Migrate to new format by writing the file
    write_remote_config(repo_path, remote_name, &url, &filter_str, &fetch, None)
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

    Ok(RemoteConfig {
        url,
        ref_spec: fetch,
        filter_with_meta: filter,
        forge,
    })
}

fn atomic_write(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    let parent = path.parent().context("Remote config path has no parent")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Remote config path is not valid UTF-8")?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = parent.join(format!(".{name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> anyhow::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Write remote configuration to .git/josh/remotes/<name>.josh file
pub fn write_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
    url: &str,
    filter: &str,
    fetch: &str,
    forge: Option<Forge>,
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

    // Wrap the filter with metadata
    let mut filter_with_meta = filter_obj.with_meta("url", url).with_meta("fetch", fetch);

    if let Some(forge) = forge {
        filter_with_meta = filter_with_meta.with_meta("forge", forge.to_string());
    }

    // Serialize the filter with metadata
    let content = josh_core::filter::as_file(filter_with_meta, 0);

    // Write to file
    let remote_file = remotes_dir.join(format!("{}.josh", remote_name));
    atomic_write(&remote_file, &content).with_context(|| {
        format!(
            "Failed to write remote config file: {}",
            remote_file.display()
        )
    })?;

    Ok(())
}
