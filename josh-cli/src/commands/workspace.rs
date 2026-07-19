use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, anyhow};
use serde::Serialize;

use crate::{cli_eprintln as eprintln, cli_println as println};

const WORKSPACE_FILE: &str = "workspace.josh";

#[derive(Debug, clap::Parser)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum WorkspaceCommand {
    /// Create a workspace definition in the current repository
    Create(WorkspaceCreateArgs),
    /// List workspace definitions in the current working tree
    List(WorkspaceListArgs),
    /// Show a workspace definition and its canonical filter
    Show(WorkspaceShowArgs),
    /// Validate one or more workspace definitions
    Validate(WorkspaceValidateArgs),
    /// Materialize a workspace as a detached local Git worktree
    Checkout(WorkspaceCheckoutArgs),
}

#[derive(Debug, clap::Parser)]
pub struct WorkspaceCreateArgs {
    /// Repository-relative directory that will contain workspace.josh
    pub path: PathBuf,

    /// Add a mapping as DESTINATION=FILTER (repeatable)
    #[arg(long = "map", value_name = "DESTINATION=FILTER")]
    pub mappings: Vec<String>,

    /// Replace an existing workspace definition
    #[arg(long)]
    pub force: bool,

    /// Validate and show the result without writing it
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, clap::Parser)]
pub struct WorkspaceListArgs {
    /// Include the canonical filter in human output
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, clap::Parser)]
pub struct WorkspaceShowArgs {
    /// Repository-relative workspace directory or workspace.josh file
    pub path: PathBuf,
}

#[derive(Debug, clap::Parser)]
pub struct WorkspaceValidateArgs {
    /// Workspaces to validate; validates every discovered workspace when omitted
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, clap::Parser)]
pub struct WorkspaceCheckoutArgs {
    /// Repository-relative workspace directory or workspace.josh file
    pub path: PathBuf,

    /// Directory in which to materialize the filtered worktree
    pub out: PathBuf,

    /// Input revision: "." (working tree), "+" (index), or any Git revision
    #[arg(short = 'r', long = "reference", default_value = ".")]
    pub reference: String,

    /// Resolve and filter the workspace without creating the worktree
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceInfo {
    pub path: String,
    pub file: String,
    pub valid: bool,
    pub reversible: bool,
    pub filter: Option<String>,
    pub filter_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateResult {
    action: &'static str,
    path: String,
    file: String,
    mappings: usize,
    dry_run: bool,
    definition: String,
    filter: String,
    filter_id: String,
}

#[derive(Debug, Serialize)]
struct CheckoutResult {
    action: &'static str,
    workspace: String,
    output: String,
    reference: String,
    commit: String,
    dry_run: bool,
    detached: bool,
}

pub fn handle_workspace(
    args: &WorkspaceArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        WorkspaceCommand::Create(args) => handle_create(args, transaction),
        WorkspaceCommand::List(args) => handle_list(args, transaction),
        WorkspaceCommand::Show(args) => handle_show(args, transaction),
        WorkspaceCommand::Validate(args) => handle_validate(args, transaction),
        WorkspaceCommand::Checkout(args) => handle_checkout(args, transaction),
    }
}

fn repo_root(repo: &git2::Repository) -> anyhow::Result<&Path> {
    repo.workdir()
        .ok_or_else(|| anyhow!("Workspace commands require a non-bare Git repository"))
}

fn normalize_workspace_path(path: &Path) -> anyhow::Result<PathBuf> {
    let path = if path.file_name().is_some_and(|name| name == WORKSPACE_FILE) {
        path.parent().unwrap_or_else(|| Path::new("."))
    } else {
        path
    };

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!(
                    "Workspace path '{}' must be relative and remain inside the repository",
                    path.display()
                ));
            }
        }
    }
    Ok(normalized)
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn display_path(path: &Path) -> String {
    let value = path.display().to_string();
    if let Ok(test_root) = std::env::var("TESTTMP") {
        value.replace(&test_root, "${TESTTMP}")
    } else {
        value
    }
}

fn workspace_name(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn workspace_file(root: &Path, path: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let workspace = normalize_workspace_path(path)?;
    Ok((workspace.clone(), root.join(workspace).join(WORKSPACE_FILE)))
}

fn validate_content(path: &Path, content: &str) -> WorkspaceInfo {
    let workspace = workspace_name(path);
    let file = if workspace == "." {
        WORKSPACE_FILE.to_string()
    } else {
        format!("{workspace}/{WORKSPACE_FILE}")
    };

    match josh_core::filter::parse(content) {
        Ok(filter) => match josh_core::filter::invert(filter) {
            Ok(_) => WorkspaceInfo {
                path: workspace,
                file,
                valid: true,
                reversible: true,
                filter: Some(josh_core::filter::pretty(filter, 0)),
                filter_id: Some(filter.id().to_string()),
                error: None,
            },
            Err(error) => WorkspaceInfo {
                path: workspace,
                file,
                valid: false,
                reversible: false,
                filter: Some(josh_core::filter::pretty(filter, 0)),
                filter_id: Some(filter.id().to_string()),
                error: Some(format!("Workspace filter is not reversible: {error}")),
            },
        },
        Err(error) => WorkspaceInfo {
            path: workspace,
            file,
            valid: false,
            reversible: false,
            filter: None,
            filter_id: None,
            error: Some(error.to_string()),
        },
    }
}

fn read_workspace(root: &Path, path: &Path) -> anyhow::Result<WorkspaceInfo> {
    let (workspace, file) = workspace_file(root, path)?;
    let content = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read workspace definition '{}'", file.display()))?;
    Ok(validate_content(&workspace, &content))
}

fn is_workspace_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == WORKSPACE_FILE)
}

fn discover_workspaces(repo: &git2::Repository) -> anyhow::Result<Vec<WorkspaceInfo>> {
    let root = repo_root(repo)?;
    let mut relative_files = std::collections::BTreeSet::new();

    // The index supplies tracked definitions without walking ignored build output directories.
    for entry in repo.index()?.iter() {
        let path = PathBuf::from(String::from_utf8_lossy(&entry.path).into_owned());
        if is_workspace_file(&path) && root.join(&path).is_file() {
            relative_files.insert(path);
        }
    }

    // Include untracked definitions so `create` is immediately visible before a commit.
    let mut options = git2::StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);
    for entry in repo.statuses(Some(&mut options))?.iter() {
        if let Some(path) = entry.path().map(PathBuf::from)
            && is_workspace_file(&path)
            && root.join(&path).is_file()
        {
            relative_files.insert(path);
        }
    }

    let mut workspaces = relative_files
        .into_iter()
        .map(|relative| {
            let path = relative.parent().unwrap_or_else(|| Path::new("."));
            let content = std::fs::read_to_string(root.join(&relative))?;
            Ok(validate_content(path, &content))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    workspaces.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(workspaces)
}

fn parse_mapping(mapping: &str) -> anyhow::Result<(String, josh_core::filter::Filter)> {
    let (destination, filter_spec) = mapping
        .split_once('=')
        .ok_or_else(|| anyhow!("Invalid mapping '{}'; expected DESTINATION=FILTER", mapping))?;
    let destination = destination.trim();
    let filter_spec = filter_spec.trim();

    if destination.is_empty()
        || destination.starts_with('/')
        || destination.ends_with('/')
        || destination
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(anyhow!(
            "Invalid workspace mapping destination '{}'",
            destination
        ));
    }
    if filter_spec.is_empty() {
        return Err(anyhow!("Mapping '{}' has an empty filter", mapping));
    }

    let filter = josh_core::filter::parse(filter_spec)
        .with_context(|| format!("Invalid filter in mapping '{}'", mapping))?;
    josh_core::filter::invert(filter)
        .with_context(|| format!("Filter in mapping '{}' is not reversible", mapping))?;
    Ok((destination.to_string(), filter))
}

fn create_content(mappings: &[String]) -> anyhow::Result<(String, josh_core::filter::Filter)> {
    if mappings.is_empty() {
        let content = "# Empty Josh workspace\n".to_string();
        let filter = josh_core::filter::parse(&content)?;
        return Ok((content, filter));
    }

    let mut lines = Vec::new();
    let mut destinations = std::collections::HashSet::new();
    for mapping in mappings {
        let (destination, filter) = parse_mapping(mapping)?;
        if !destinations.insert(destination.clone()) {
            return Err(anyhow!(
                "Duplicate workspace mapping destination '{}'",
                destination
            ));
        }
        lines.push(format!(
            "{} = {}",
            destination,
            josh_core::filter::spec(filter)
        ));
    }
    let content = format!("{}\n", lines.join("\n"));
    let filter = josh_core::filter::parse(&content)?;
    josh_core::filter::invert(filter).context("Workspace filter is not reversible")?;
    Ok((content, filter))
}

fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    let parent = path.parent().context("Workspace file has no parent")?;
    std::fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = parent.join(format!(
        ".{WORKSPACE_FILE}.tmp-{}-{nonce}",
        std::process::id()
    ));

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

fn handle_create(
    args: &WorkspaceCreateArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let root = repo_root(transaction.repo())?;
    let (workspace, file) = workspace_file(root, &args.path)?;
    let existed = file.exists();
    if existed && !args.force {
        return Err(anyhow!(
            "Workspace '{}' already exists; pass --force to replace it",
            workspace_name(&workspace)
        ));
    }

    let (content, filter) = create_content(&args.mappings)?;
    if !args.dry_run {
        atomic_write(&file, &content)
            .with_context(|| format!("Failed to write workspace '{}'", file.display()))?;
    }

    let relative_file = file.strip_prefix(root).unwrap_or(&file);
    let result = CreateResult {
        action: if existed { "replace" } else { "create" },
        path: workspace_name(&workspace),
        file: relative_file.to_string_lossy().replace('\\', "/"),
        mappings: args.mappings.len(),
        dry_run: args.dry_run,
        definition: content.trim_end().to_string(),
        filter: josh_core::filter::pretty(filter, 0),
        filter_id: filter.id().to_string(),
    };
    crate::output::set_data(&result)?;

    if args.dry_run {
        println!("Would create workspace '{}'", result.path);
    } else {
        println!("Created workspace '{}'", result.path);
    }
    println!("Definition: {}", result.file);
    if !args.mappings.is_empty() {
        println!("\n{}", result.definition);
    }
    Ok(())
}

fn handle_list(
    args: &WorkspaceListArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let workspaces = discover_workspaces(transaction.repo())?;
    crate::output::set_data(&workspaces)?;

    if workspaces.is_empty() {
        println!("No workspaces found.");
        return Ok(());
    }

    for workspace in &workspaces {
        println!(
            "{}\t{}",
            if workspace.valid { "valid" } else { "invalid" },
            workspace.path
        );
        if args.verbose {
            if let Some(filter) = &workspace.filter {
                println!("{}", indent(filter, "  "));
            }
            if let Some(error) = &workspace.error {
                println!("  Error: {error}");
            }
        }
    }
    Ok(())
}

fn indent(value: &str, prefix: &str) -> String {
    value
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn handle_show(
    args: &WorkspaceShowArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let workspace = read_workspace(repo_root(transaction.repo())?, &args.path)?;
    crate::output::set_data(&workspace)?;

    println!("Workspace: {}", workspace.path);
    println!("Definition: {}", workspace.file);
    println!(
        "Status: {}",
        if workspace.valid { "valid" } else { "invalid" }
    );
    if let Some(filter) = &workspace.filter {
        println!("Filter:\n{}", indent(filter, "  "));
    }
    if let Some(error) = &workspace.error {
        return Err(anyhow!("Invalid workspace '{}': {error}", workspace.path));
    }
    Ok(())
}

fn handle_validate(
    args: &WorkspaceValidateArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let root = repo_root(transaction.repo())?;
    let workspaces = if args.paths.is_empty() {
        discover_workspaces(transaction.repo())?
    } else {
        args.paths
            .iter()
            .map(|path| read_workspace(root, path))
            .collect::<anyhow::Result<Vec<_>>>()?
    };
    crate::output::set_data(&workspaces)?;

    if workspaces.is_empty() {
        println!("No workspaces found.");
        return Ok(());
    }

    let mut invalid = 0;
    for workspace in &workspaces {
        if workspace.valid {
            println!("valid\t{}", workspace.path);
        } else {
            invalid += 1;
            println!("invalid\t{}", workspace.path);
            if let Some(error) = &workspace.error {
                eprintln!("Warning: {}: {error}", workspace.path);
            }
        }
    }

    if invalid > 0 {
        return Err(anyhow!("{invalid} workspace definition(s) are invalid"));
    }
    Ok(())
}

fn handle_checkout(
    args: &WorkspaceCheckoutArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let root = repo_root(repo)?;
    let (workspace_path, _) = workspace_file(root, &args.path)?;
    let workspace = read_workspace(root, &workspace_path)?;
    if !workspace.valid {
        return Err(anyhow!(
            "Invalid workspace '{}': {}",
            workspace.path,
            workspace.error.as_deref().unwrap_or("unknown error")
        ));
    }

    let input = josh_core::git::resolve_snapshot_input(repo, &args.reference)
        .with_context(|| format!("Failed to resolve input '{}'", args.reference))?;
    let filter = josh_core::filter::Filter::new().workspace(&workspace_path);
    let filtered = josh_core::filter_commit(transaction, filter, input)
        .context("Failed to materialize workspace")?;
    if filtered == git2::Oid::zero() {
        return Err(anyhow!(
            "Workspace '{}' produced an empty history",
            workspace.path
        ));
    }

    let output = normalize_absolute_path(&if args.out.is_absolute() {
        args.out.clone()
    } else {
        std::env::current_dir()?.join(&args.out)
    });
    if output.exists() && !args.dry_run {
        return Err(anyhow!(
            "Checkout destination '{}' already exists",
            output.display()
        ));
    }

    let result = CheckoutResult {
        action: "checkout",
        workspace: workspace.path.clone(),
        output: display_path(&output),
        reference: args.reference.clone(),
        commit: filtered.to_string(),
        dry_run: args.dry_run,
        detached: true,
    };
    crate::output::set_data(&result)?;

    if args.dry_run {
        println!(
            "Would check out workspace '{}' at {}",
            workspace.path, result.output
        );
    } else {
        let output_arg = output.to_string_lossy();
        let filtered_arg = filtered.to_string();
        transaction
            .spawn_git(
                &["worktree", "add", "--detach", &output_arg, &filtered_arg],
                &[],
            )
            .context("Failed to create workspace worktree")?;
        println!(
            "Checked out workspace '{}' at {}",
            workspace.path, result.output
        );
        eprintln!(
            "This is a detached local view; use 'josh clone ... :workspace={} ...' for a \
             bidirectional remote checkout.",
            workspace.path
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_validation() {
        assert!(parse_mapping("src=:/apps/frontend").is_ok());
        assert!(parse_mapping("../src=:/apps/frontend").is_err());
        assert!(parse_mapping("src").is_err());
        assert!(parse_mapping("src=").is_err());
    }

    #[test]
    fn empty_workspace_is_valid() {
        let (content, filter) = create_content(&[]).unwrap();
        assert_eq!(content, "# Empty Josh workspace\n");
        assert!(josh_core::filter::invert(filter).is_ok());
    }

    #[test]
    fn workspace_path_cannot_escape_repository() {
        assert_eq!(
            normalize_workspace_path(Path::new("ws/app")).unwrap(),
            Path::new("ws/app")
        );
        assert!(normalize_workspace_path(Path::new("../app")).is_err());
        assert!(normalize_workspace_path(Path::new("/app")).is_err());
    }
}
