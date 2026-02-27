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
    /// Push the linked repository to its remote using the :export filter
    Push(LinkPushArgs),
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

#[derive(Debug, clap::Parser)]
pub struct LinkPushArgs {
    /// Path of the link to push (e.g. /docs or docs)
    #[arg()]
    pub path: String,
}

pub fn handle_link(
    args: &LinkArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args, transaction),
        LinkCommand::Fetch(fetch_args) => handle_link_fetch(fetch_args, transaction),
        LinkCommand::Update(update_args) => handle_link_update(update_args, transaction),
        LinkCommand::Push(push_args) => handle_link_push(push_args, transaction),
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

    // Get the current HEAD commit
    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let mode = josh_core::filter::LinkMode::parse(&args.mode)
        .with_context(|| format!("Invalid link mode: '{}'", args.mode))?;

    // Compute the initial commit OID using the :export filter, the same way
    // as `josh link push` does, so the stored commit reflects the local state.
    let normalized_path = args.path.trim_matches('/');
    let path_filter = josh_core::filter::Filter::new().subdir(normalized_path);
    let filter_obj = josh_core::filter::parse(filter)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;
    let combined_filter = path_filter.export().chain(
        josh_core::filter::invert(filter_obj)
            .with_context(|| format!("Filter '{}' has no inverse", filter))?,
    );
    let export_oid = josh_core::filter_commit(transaction, combined_filter, head_commit.id())
        .context("Failed to apply export filter")?;

    // If the export filter found no local content, fall back to fetching the remote.
    let initial_oid = if export_oid != git2::Oid::zero() {
        eprintln!(
            "Using local content at '{}' ({})",
            normalized_path, export_oid
        );
        export_oid
    } else {
        eprintln!(
            "No local content at '{}', fetching from remote...",
            normalized_path
        );

        josh_core::git::spawn_git_command(repo.path(), &["fetch", &args.url, target], &[])
            .context("Failed to execute git fetch")?;

        let fetched_oid = repo
            .find_reference("FETCH_HEAD")
            .context("Failed to find FETCH_HEAD after fetch")?
            .peel_to_commit()
            .context("Failed to peel FETCH_HEAD to commit")?
            .id();

        eprintln!("Using fetched commit {}", fetched_oid);
        fetched_oid
    };

    // Create a new commit with the updated tree
    let signature = make_signature(&repo)?;

    let commit_oid = josh_link::prepare_link_add(
        transaction,
        std::path::Path::new(&args.path),
        &args.url,
        args.filter.as_deref(),
        target,
        initial_oid,
        &head_tree,
        mode,
    )?
    .into_commit(transaction, &head_commit, &signature)?;

    // Create the fixed branch name
    let branch_name = "refs/heads/josh-link";

    // Create or update the branch reference
    repo.reference(branch_name, commit_oid, true, "josh link add")
        .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

    eprintln!(
        "Added link '{}' with URL '{}', filter '{}', target '{}', and mode '{}'",
        normalized_path, args.url, filter, target, args.mode
    );
    eprintln!("Created branch: {}", branch_name);

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
        eprintln!("No .link.josh references found in history");
        return Ok(());
    }

    eprintln!(
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

        eprintln!("Fetching {} from {}", link_ref.commit, link_ref.remote);

        josh_core::git::spawn_git_command(repo.path(), &["fetch", &link_ref.remote, &refspec], &[])
            .with_context(|| {
                format!(
                    "git fetch of {} from {} failed",
                    link_ref.commit, link_ref.remote
                )
            })?;

        fetched += 1;
    }

    eprintln!(
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

    eprintln!("Found {} link file(s) to update", link_files.len());

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

        eprintln!("Fetching {} from {}", branch, remote);

        josh_core::git::spawn_git_command(repo.path(), &["fetch", &remote, &branch], &[])
            .with_context(|| format!("git fetch failed for '{}'", path.display()))?;

        let new_oid = repo
            .find_reference("FETCH_HEAD")
            .context("Failed to find FETCH_HEAD")?
            .peel_to_commit()
            .context("Failed to get FETCH_HEAD commit")?
            .id();

        links_to_update.push((path.clone(), new_oid));
    }

    let signature = make_signature(&repo)?;
    let Some(result) = josh_link::update_links(
        &repo,
        transaction,
        &head_commit,
        links_to_update,
        &signature,
    )?
    else {
        eprintln!("All {} link file(s) already up to date", link_files.len());
        return Ok(());
    };

    let branch_name = "refs/heads/josh-link";
    repo.reference(
        branch_name,
        result.filtered_commit,
        true,
        "josh link update",
    )
    .with_context(|| format!("Failed to update branch '{}'", branch_name))?;

    eprintln!("Updated {} link file(s)", link_files.len());
    eprintln!("Updated branch: {}", branch_name);

    Ok(())
}

fn handle_link_push(
    args: &LinkPushArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Get current HEAD commit
    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Normalize path: strip slash(es)
    let normalized_path = args.path.trim_matches('/');
    if normalized_path.is_empty() {
        return Err(anyhow!("Path cannot be empty"));
    }

    // Find the .link.josh file at the given path
    let link_files =
        josh_core::link::find_link_files(&repo, &head_tree).context("Failed to find link files")?;

    let link_path = std::path::PathBuf::from(normalized_path);
    let (_, link_file) = link_files
        .iter()
        .find(|(p, _)| p == &link_path)
        .ok_or_else(|| anyhow!("No link found at path '{}'", args.path))?;

    let remote = link_file
        .get_meta("remote")
        .ok_or_else(|| anyhow!("Link file missing 'remote' metadata"))?;
    let target = link_file
        .get_meta("target")
        .unwrap_or_else(|| "HEAD".to_string());

    // Build the export filter: subdir extracts the link path, :export strips .link.josh
    let path_filter = josh_core::filter::Filter::new().subdir(normalized_path);
    let combined_filter = path_filter
        .export()
        .chain(josh_core::filter::invert(*link_file)?);

    // Apply the filter to get the commit suitable for pushing
    let exported_commit = josh_core::filter_commit(transaction, combined_filter, head_commit.id())
        .context("Failed to apply export filter")?;

    if exported_commit == git2::Oid::zero() {
        return Err(anyhow!("No content found at path '{}' to push", args.path));
    }

    // Determine the destination ref: treat "HEAD" as "master" for push
    // FIXME: this needs to properly resolve the HEAD symref
    let push_ref = if target == "HEAD" {
        "refs/heads/master".to_string()
    } else {
        target.clone()
    };
    let refspec = format!("{}:{}", exported_commit, push_ref);

    josh_core::git::spawn_git_command(repo.path(), &["push", &remote, &refspec], &[])
        .with_context(|| format!("Failed to push to '{}'", remote))?;

    Ok(())
}
