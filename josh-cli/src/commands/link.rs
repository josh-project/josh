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
    /// Fetch all SHAs referenced in .link.josh files across history
    Fetch(LinkFetchArgs),
    /// Fetch the latest commit from each linked remote and update .link.josh files
    Update(LinkUpdateArgs),
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

    /// Link mode: embedded, snapshot, or pointer (defaults to snapshot)
    #[arg(long = "mode", default_value = "snapshot")]
    pub mode: String,
}

#[derive(Debug, clap::Parser)]
pub struct LinkFetchArgs {
    /// Josh filter selecting which links to consider (considers all if omitted)
    #[arg()]
    pub filter: Option<String>,
}

#[derive(Debug, clap::Parser)]
pub struct LinkUpdateArgs {
    /// Josh filter selecting which links to update (updates all if omitted)
    #[arg()]
    pub filter: Option<String>,
}

pub fn handle_link(
    args: &LinkArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args, transaction),
        LinkCommand::Fetch(fetch_args) => handle_link_fetch(fetch_args, transaction),
        LinkCommand::Update(update_args) => handle_link_update(update_args, transaction),
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

    let mode = josh_core::filter::LinkMode::parse(&args.mode)
        .with_context(|| format!("Invalid link mode: '{}'", args.mode))?;

    let result = josh_link::prepare_link_add(
        transaction,
        std::path::Path::new(&args.path),
        &args.url,
        args.filter.as_deref(),
        target,
        fetched_commit,
        &head_tree,
        mode,
    )?
    .into_commit(transaction, &head_commit, &signature)?;

    // Create the fixed branch name
    let branch_name = "refs/heads/josh-link";

    // Create or update the branch reference
    repo.reference(branch_name, result.filtered_commit, true, "josh link add")
        .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

    let normalized_path = args.path.trim_matches('/');
    println!(
        "Added link '{}' with URL '{}', filter '{}', target '{}', and mode '{}'",
        normalized_path, args.url, filter, target, args.mode
    );
    println!("Created branch: {}", branch_name);

    Ok(())
}

fn handle_link_fetch(
    args: &LinkFetchArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;

    let commit_oid = if let Some(filter_str) = &args.filter {
        let filter = josh_core::filter::parse(filter_str)
            .with_context(|| format!("Failed to parse filter '{}'", filter_str))?;
        let roundtrip = filter.chain(
            josh_core::filter::invert(filter)
                .with_context(|| format!("Filter '{}' has no inverse", filter_str))?,
        );
        josh_core::filter_commit(transaction, roundtrip, head_commit.id())
            .context("Failed to apply filter")?
    } else {
        head_commit.id()
    };

    let link_refs = josh_link::collect_all_link_refs(transaction, commit_oid)
        .context("Failed to collect link refs from history")?;

    if link_refs.is_empty() {
        println!("No .link.josh references found in history");
        return Ok(());
    }

    println!(
        "Found {} unique (remote, sha) pair(s) across history",
        link_refs.len()
    );

    let odb = repo.odb().context("Failed to open object database")?;

    let mut fetched = 0;
    let mut skipped = 0;

    for link_ref in &link_refs {
        let oid = git2::Oid::from_str(&link_ref.commit)
            .with_context(|| format!("Invalid commit SHA in link file: {}", link_ref.commit))?;

        if odb.exists(oid) {
            skipped += 1;
            continue;
        }

        // Fetch the specific SHA from the remote into a temporary ref so the
        // object is stored in the local ODB.
        let refspec = format!(
            "{}:refs/josh/link-shas/{}",
            link_ref.commit, link_ref.commit
        );

        println!("Fetching {} from {}", link_ref.commit, link_ref.remote);

        josh_core::git::spawn_git_command(repo.path(), &["fetch", &link_ref.remote, &refspec], &[])
            .with_context(|| {
                format!(
                    "git fetch of {} from {} failed",
                    link_ref.commit, link_ref.remote
                )
            })?;

        fetched += 1;
    }

    println!(
        "Done: fetched {}, skipped {} (already present)",
        fetched, skipped
    );

    Ok(())
}

fn handle_link_update(
    args: &LinkUpdateArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files = if let Some(filter_str) = &args.filter {
        let filter = josh_core::filter::parse(filter_str)
            .with_context(|| format!("Failed to parse filter '{}'", filter_str))?;
        let roundtrip = filter.chain(
            josh_core::filter::invert(filter)
                .with_context(|| format!("Filter '{}' has no inverse", filter_str))?,
        );
        let filtered_oid = josh_core::filter_commit(transaction, roundtrip, head_commit.id())
            .context("Failed to apply filter")?;
        if filtered_oid == git2::Oid::zero() {
            vec![]
        } else {
            let filtered_tree = repo
                .find_commit(filtered_oid)
                .context("Failed to find filtered commit")?
                .tree()
                .context("Failed to get filtered tree")?;
            josh_core::link::find_link_files(&repo, &filtered_tree)
                .context("Failed to find link files in filtered tree")?
        }
    } else {
        josh_core::link::find_link_files(&repo, &head_tree).context("Failed to find link files")?
    };

    if link_files.is_empty() {
        return Err(anyhow!("No .link.josh files found"));
    }

    println!("Found {} link file(s) to update", link_files.len());

    let mut links_to_update = Vec::new();
    for (path, link_file) in &link_files {
        let remote = link_file.get_meta("remote").ok_or_else(|| {
            anyhow!(
                "Link file missing 'remote' metadata at path '{}'",
                path.display()
            )
        })?;
        let branch = link_file
            .get_meta("target")
            .unwrap_or_else(|| "HEAD".to_string());

        println!("Fetching {} from {}", branch, remote);

        let output = std::process::Command::new("git")
            .args(&["fetch", &remote, &branch])
            .output()
            .context("Failed to execute git fetch")?;

        if !output.status.success() {
            return Err(anyhow!(
                "git fetch failed for '{}': {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let new_oid = repo
            .find_reference("FETCH_HEAD")
            .context("Failed to find FETCH_HEAD")?
            .peel_to_commit()
            .context("Failed to get FETCH_HEAD commit")?
            .id();

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

    let branch_name = "refs/heads/josh-link";
    repo.reference(
        branch_name,
        result.filtered_commit,
        true,
        "josh link update",
    )
    .with_context(|| format!("Failed to update branch '{}'", branch_name))?;

    println!("Updated {} link file(s)", link_files.len());
    println!("Updated branch: {}", branch_name);

    Ok(())
}
