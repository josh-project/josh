use anyhow::Context;
use serde::Serialize;

use crate::cli_println as println;
use crate::config::list_remote_names;

#[derive(Debug, clap::Parser)]
pub struct StatusArgs {}

#[derive(Debug, Serialize)]
struct WorkingTreeStatus {
    clean: bool,
    staged: usize,
    modified: usize,
    untracked: usize,
    conflicted: usize,
}

#[derive(Debug, Serialize)]
struct RemoteStatus {
    name: String,
    url: String,
    filter: String,
    forge: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusResult {
    repository: String,
    branch: Option<String>,
    detached: bool,
    head: Option<String>,
    working_tree: WorkingTreeStatus,
    remotes: Vec<RemoteStatus>,
}

pub fn handle_status(_args: &StatusArgs, repo: &git2::Repository) -> anyhow::Result<()> {
    let workdir = repo
        .workdir()
        .context("Status requires a non-bare Git repository")?;
    let head = repo.head().ok();
    let detached = repo.head_detached().unwrap_or(false);
    let branch = if detached {
        None
    } else {
        head.as_ref()
            .and_then(|reference| reference.shorthand())
            .map(ToOwned::to_owned)
    };
    let head_oid = head
        .and_then(|reference| reference.peel_to_commit().ok())
        .map(|commit| commit.id().to_string());

    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut options))?;

    let mut working_tree = WorkingTreeStatus {
        clean: statuses.is_empty(),
        staged: 0,
        modified: 0,
        untracked: 0,
        conflicted: 0,
    };
    for entry in statuses.iter() {
        let status = entry.status();
        if status.is_conflicted() {
            working_tree.conflicted += 1;
        }
        if status.is_index_new()
            || status.is_index_modified()
            || status.is_index_deleted()
            || status.is_index_renamed()
            || status.is_index_typechange()
        {
            working_tree.staged += 1;
        }
        if status.is_wt_modified()
            || status.is_wt_deleted()
            || status.is_wt_renamed()
            || status.is_wt_typechange()
        {
            working_tree.modified += 1;
        }
        if status.is_wt_new() {
            working_tree.untracked += 1;
        }
    }

    let mut remotes = Vec::new();
    for name in list_remote_names(workdir)? {
        if let Ok(config) = crate::config::read_remote_config(workdir, &name) {
            remotes.push(RemoteStatus {
                name,
                url: crate::output::sanitize(&config.url),
                filter: josh_core::filter::spec(config.filter_with_meta.peel()),
                forge: config.forge.map(|forge| forge.to_string()),
            });
        }
    }

    let result = StatusResult {
        repository: crate::output::sanitize(&workdir.display().to_string()),
        branch,
        detached,
        head: head_oid,
        working_tree,
        remotes,
    };
    crate::output::set_data(&result)?;

    println!("Repository: {}", result.repository);
    println!(
        "Branch: {}",
        result.branch.as_deref().unwrap_or(if result.detached {
            "(detached)"
        } else {
            "(unborn)"
        })
    );
    println!(
        "Working tree: {}",
        if result.working_tree.clean {
            "clean".to_string()
        } else {
            format!(
                "{} staged, {} modified, {} untracked, {} conflicted",
                result.working_tree.staged,
                result.working_tree.modified,
                result.working_tree.untracked,
                result.working_tree.conflicted
            )
        }
    );
    if result.remotes.is_empty() {
        println!("Josh remotes: none");
    } else {
        println!("Josh remotes:");
        for remote in &result.remotes {
            println!("  {}  {}  {}", remote.name, remote.filter, remote.url);
        }
    }
    Ok(())
}
