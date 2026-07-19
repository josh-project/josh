use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use serde::Serialize;

use crate::cli_println as println;

pub const SKILL_NAME: &str = "josh";
pub const SKILL_VERSION: &str = "1";
pub const SKILL_CONTENT: &str = include_str!("../../skills/josh/SKILL.md");

#[derive(Debug, clap::Parser)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum AgentCommand {
    /// Manage the bundled Josh agent skill
    Skill(SkillArgs),
}

#[derive(Debug, clap::Parser)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum SkillCommand {
    /// Print the bundled SKILL.md to stdout
    Print(SkillPrintArgs),
    /// Install the bundled skill into an agent skill directory
    Install(SkillInstallArgs),
}

#[derive(Debug, clap::Parser)]
pub struct SkillPrintArgs {}

#[derive(Debug, clap::Parser)]
pub struct SkillInstallArgs {
    /// Skill directory to create or update
    #[arg(long, default_value = ".agents/skills/josh")]
    pub target: PathBuf,

    /// Replace an existing SKILL.md
    #[arg(long)]
    pub force: bool,

    /// Show the destination without writing files
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
struct PrintedSkill<'a> {
    name: &'static str,
    version: &'static str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct InstalledSkill {
    name: &'static str,
    version: &'static str,
    directory: String,
    file: String,
    installed: bool,
    replaced: bool,
    dry_run: bool,
}

pub fn handle_agent(args: &AgentArgs) -> anyhow::Result<()> {
    match &args.command {
        AgentCommand::Skill(args) => match &args.command {
            SkillCommand::Print(args) => handle_print(args),
            SkillCommand::Install(args) => handle_install(args),
        },
    }
}

fn handle_print(_args: &SkillPrintArgs) -> anyhow::Result<()> {
    crate::output::set_data(&PrintedSkill {
        name: SKILL_NAME,
        version: SKILL_VERSION,
        content: SKILL_CONTENT,
    })?;

    if !crate::output::is_machine()
        && let Err(error) = crate::output::raw_stdout(SKILL_CONTENT)
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(error).context("Failed to print Josh agent skill");
    }
    Ok(())
}

fn absolute_target(target: &Path) -> anyhow::Result<PathBuf> {
    if target.is_absolute() {
        Ok(target.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(target))
    }
}

fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("Skill file has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = parent.join(format!(".SKILL.md.tmp-{}-{nonce}", std::process::id()));

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

fn handle_install(args: &SkillInstallArgs) -> anyhow::Result<()> {
    let target = absolute_target(&args.target)?;
    let file = target.join("SKILL.md");
    let replaced = file.exists();
    if replaced && !args.force {
        return Err(anyhow!(
            "Agent skill '{}' already exists; pass --force to replace it",
            crate::output::sanitize(&file.display().to_string())
        ));
    }

    if !args.dry_run {
        atomic_write(&file, SKILL_CONTENT)
            .with_context(|| format!("Failed to install agent skill at '{}'", file.display()))?;
    }

    let result = InstalledSkill {
        name: SKILL_NAME,
        version: SKILL_VERSION,
        directory: crate::output::sanitize(&target.display().to_string()),
        file: crate::output::sanitize(&file.display().to_string()),
        installed: !args.dry_run,
        replaced,
        dry_run: args.dry_run,
    };
    crate::output::set_data(&result)?;

    if args.dry_run {
        println!("Would install Josh agent skill at {}", result.file);
    } else if replaced {
        println!("Updated Josh agent skill at {}", result.file);
    } else {
        println!("Installed Josh agent skill at {}", result.file);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skill_has_agent_skills_frontmatter() {
        assert!(SKILL_CONTENT.starts_with("---\nname: josh\n"));
        assert!(SKILL_CONTENT.contains("\ndescription:"));
        assert!(SKILL_CONTENT.ends_with('\n'));
    }
}
