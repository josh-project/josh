use anyhow::{Context, anyhow};

use josh_link::make_signature;

#[derive(Debug, clap::Parser)]
pub struct LinkArgs {
    /// Link subcommand
    #[command(subcommand)]
    pub command: LinkCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum LinkCommand {
    /// Add a link with optional filter and target branch
    Add(LinkAddArgs),
    /// Fetch from existing link files
    Fetch(LinkFetchArgs),
}

#[derive(Debug, clap::Parser)]
pub struct LinkAddArgs {
    /// Path where the link will be mounted
    #[arg()]
    pub path: String,

    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Optional filter to apply to the linked repository
    #[arg()]
    pub filter: Option<String>,

    /// Target branch to link (defaults to HEAD)
    #[arg(long = "target")]
    pub target: Option<String>,
}

#[derive(Debug, clap::Parser)]
pub struct LinkFetchArgs {
    /// Optional path to specific .link.josh file (if not provided, fetches all)
    #[arg()]
    pub path: Option<String>,
}

pub fn handle_link(
    args: &LinkArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args, transaction),
        LinkCommand::Fetch(fetch_args) => handle_link_fetch(fetch_args, transaction),
    }
}

fn handle_link_add(
    args: &LinkAddArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Validate the path (should not be empty and should be a valid path)
    if args.path.is_empty() {
        return Err(anyhow!("Path cannot be empty"));
    }

    // Get the filter (default to ":/" if not provided)
    let filter = args.filter.as_deref().unwrap_or(":/");

    // Get the target branch (default to "HEAD" if not provided)
    let target = args.target.as_deref().unwrap_or("HEAD");

    // Use git fetch shell command
    let output = std::process::Command::new("git")
        .args(&["fetch", &args.url, target])
        .output()
        .context("Failed to execute git fetch")?;

    if !output.status.success() {
        return Err(anyhow!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Get the commit SHA from FETCH_HEAD
    let fetch_head = repo
        .find_reference("FETCH_HEAD")
        .context("Failed to find FETCH_HEAD")?;
    let fetch_commit = fetch_head
        .peel_to_commit()
        .context("Failed to get FETCH_HEAD commit")?;
    let fetched_commit = fetch_commit.id();

    // Get the current HEAD commit
    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Create a new commit with the updated tree
    let signature = make_signature(&repo)?;

    let result = josh_link::prepare_link_add(
        transaction,
        std::path::Path::new(&args.path),
        &args.url,
        args.filter.as_deref(),
        target,
        fetched_commit,
        &head_tree,
    )?
    .into_commit(transaction, &head_commit, &signature)?;

    // Create the fixed branch name
    let branch_name = "refs/heads/josh-link";

    // Create or update the branch reference
    repo.reference(branch_name, result.filtered_commit, true, "josh link add")
        .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

    let normalized_path = args.path.trim_matches('/');
    println!(
        "Added link '{}' with URL '{}', filter '{}', and target '{}'",
        normalized_path, args.url, filter, target
    );
    println!("Created branch: {}", branch_name);

    Ok(())
}

fn handle_link_fetch(
    args: &LinkFetchArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Get the current HEAD commit
    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files = if let Some(path) = &args.path {
        // Single path specified - use find_link_files to get all link files, then find the one at the specified path
        let link_files = josh_core::link::find_link_files(&repo, &head_tree)
            .context("Failed to find link files")?;

        let link_file = link_files
            .iter()
            .find(|(p, _)| p.to_string_lossy() == path.as_str())
            .map(|(_, lf)| lf.clone())
            .ok_or_else(|| anyhow!("Link file not found at path '{}'", path))?;

        vec![(std::path::PathBuf::from(path), link_file)]
    } else {
        // No path specified - find all .link.josh files in the tree
        josh_core::link::find_link_files(&repo, &head_tree).context("Failed to find link files")?
    };

    if link_files.is_empty() {
        return Err(anyhow!("No .link.josh files found"));
    }

    println!("Found {} link file(s) to fetch", link_files.len());

    // Fetch from all the link files and collect (path, new_oid) pairs
    let mut links_to_update = Vec::new();
    for (path, link_file) in &link_files {
        println!("Fetching from link at path: {}", path.display());

        // Get remote and branch from metadata
        let remote = link_file.get_meta("remote").ok_or_else(|| {
            anyhow!(
                "Link file missing 'remote' metadata at path '{}'",
                path.display()
            )
        })?;
        let branch = link_file
            .get_meta("target")
            .unwrap_or_else(|| "HEAD".to_string());

        // Use git fetch shell command
        let output = std::process::Command::new("git")
            .args(&["fetch", &remote, &branch])
            .output()
            .context("Failed to execute git fetch")?;

        if !output.status.success() {
            return Err(anyhow!(
                "git fetch failed for path '{}': {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Get the commit SHA from FETCH_HEAD
        let fetch_head = repo
            .find_reference("FETCH_HEAD")
            .context("Failed to find FETCH_HEAD")?;
        let fetch_commit = fetch_head
            .peel_to_commit()
            .context("Failed to get FETCH_HEAD commit")?;
        let new_oid = fetch_commit.id();

        links_to_update.push((path.clone(), new_oid));
    }

    let signature = make_signature(&repo)?;
    let result = josh_link::update_links(
        &repo,
        transaction,
        &head_commit,
        links_to_update,
        &signature,
    )?;

    // Create the fixed branch name
    let branch_name = "refs/heads/josh-link";

    // Create or update the branch reference
    repo.reference(branch_name, result.filtered_commit, true, "josh link fetch")
        .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

    println!("Updated {} link file(s)", link_files.len());
    println!("Created branch: {}", branch_name);

    Ok(())
}
