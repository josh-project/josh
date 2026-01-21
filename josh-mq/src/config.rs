use anyhow::anyhow;
use gix::object::tree::EntryKind;

use crate::cli::RemoteCommands;
use crate::{Config, Remote};

pub const METAREPO_MAIN: &str = "refs/heads/main";
pub const MQ_CONFIG: &str = ".mq.toml";

pub fn read_config(repo: &gix::Repository) -> anyhow::Result<Config> {
    let main = repo.find_reference(METAREPO_MAIN)?.peel_to_commit()?;
    let tree = main.tree()?;
    let entry = tree
        .lookup_entry_by_path(MQ_CONFIG)?
        .ok_or_else(|| anyhow!("Config file not found"))?;

    let blob = entry.object()?.into_blob();
    let config_str = std::str::from_utf8(blob.data.as_ref())?;
    let config: Config = toml::from_str(config_str)?;

    Ok(config)
}

fn config_to_blob(repo: &gix::Repository, config: Config) -> anyhow::Result<gix::ObjectId> {
    let config = toml::to_string_pretty(&config)?;
    let blob = repo.write_blob(config.as_bytes())?;
    Ok(blob.into())
}

pub fn update_config(repo: &gix::Repository, config: Config, message: &str) -> anyhow::Result<()> {
    let main = repo.find_reference(METAREPO_MAIN)?.peel_to_commit()?;
    let config_blob = config_to_blob(repo, config)?;
    let mut new_tree = main.tree()?.edit()?;
    new_tree.upsert(MQ_CONFIG, EntryKind::Blob, config_blob)?;
    let new_tree = new_tree.write()?;

    repo.commit(METAREPO_MAIN, message, new_tree, [main.id])?;

    Ok(())
}

pub fn create_empty_config(repo: &gix::Repository) -> anyhow::Result<()> {
    let main = repo.find_reference(METAREPO_MAIN)?.peel_to_commit()?;
    let existing = main.tree()?.lookup_entry_by_path(MQ_CONFIG)?;

    if existing.is_some() {
        return Err(anyhow!("Repo already initialized"));
    }

    update_config(repo, Config::default(), "Init metarepo")?;

    Ok(())
}

pub fn handle_config_remote_command(
    repo: &gix::Repository,
    command: RemoteCommands,
) -> anyhow::Result<()> {
    match command {
        RemoteCommands::Add {
            name,
            url,
            main,
            credential,
        } => {
            let mut config = read_config(repo)?;
            if config.remotes.contains_key(&name) {
                return Err(anyhow!("Remote '{}' already exists", name));
            }
            config.remotes.insert(
                name.clone(),
                Remote {
                    url,
                    main,
                    credential,
                },
            );
            update_config(repo, config, &format!("Add remote '{}'", name))?;
            eprintln!("Added remote '{}'", name);
        }
        RemoteCommands::Remove { name } => {
            let mut config = read_config(repo)?;
            if config.remotes.remove(&name).is_none() {
                return Err(anyhow!("Remote '{}' not found", name));
            }
            update_config(repo, config, &format!("Remove remote '{}'", name))?;
            eprintln!("Removed remote '{}'", name);
        }
    }

    Ok(())
}
