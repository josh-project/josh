#![warn(unused_extern_crates)]

use anyhow::{Context, anyhow};
use clap::Parser;

use josh_core::changes::{PushMode, build_to_push};
use josh_core::git::{normalize_repo_path, spawn_git_command};

#[derive(Debug, clap::Parser)]
#[command(name = "josh", version, about = "Josh: Git projections & sync tooling", long_about = None)]
pub struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Clone a repository with optional projection/filtering
    Clone(CloneArgs),

    /// Fetch from a remote (like `git fetch`) with projection-aware options
    Fetch(FetchArgs),

    /// Fetch & integrate from a remote (like `git pull`) with projection-aware options
    Pull(PullArgs),

    /// Push refs to a remote (like `git push`) with projection-aware options
    Push(PushArgs),

    /// Add a remote with optional projection/filtering (like `git remote add`)
    Remote(RemoteArgs),

    /// Apply filtering to existing refs (like `josh fetch` but without fetching)
    Filter(FilterArgs),

    /// Manage josh links (like `josh remote` but for links)
    #[cfg(feature = "incubating")]
    Link(LinkArgs),
}

#[derive(Debug, clap::Parser)]
pub struct CloneArgs {
    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Workspace/projection identifier or path to spec
    #[arg()]
    pub filter: String,

    /// Checkout directory
    #[arg()]
    pub out: std::path::PathBuf,

    /// Branch or ref to clone
    #[arg(short = 'b', long = "branch", default_value = "HEAD")]
    pub branch: String,

    /// Keep trivial merges (don't append :prune=trivial-merge to filters)
    #[arg(long = "keep-trivial-merges")]
    pub keep_trivial_merges: bool,
}

#[derive(Debug, clap::Parser)]
pub struct PullArgs {
    /// Remote name (or URL) to pull from
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// Ref to pull (branch, tag, or commit-ish)
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,

    /// Prune tracking refs no longer on the remote
    #[arg(long = "prune", action = clap::ArgAction::SetTrue)]
    pub prune: bool,

    /// Fast-forward only (fail if merge needed)
    #[arg(long = "ff-only", action = clap::ArgAction::SetTrue)]
    pub ff_only: bool,

    /// Rebase the current branch on top of the upstream branch
    #[arg(long = "rebase", action = clap::ArgAction::SetTrue)]
    pub rebase: bool,

    /// Automatically stash local changes before rebase
    #[arg(long = "autostash", action = clap::ArgAction::SetTrue)]
    pub autostash: bool,
}

#[derive(Debug, clap::Parser)]
pub struct FetchArgs {
    /// Remote name (or URL) to fetch from
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// Ref to fetch (branch, tag, or commit-ish)
    #[arg(short = 'R', long = "ref", default_value = "HEAD")]
    pub rref: String,

    /// Prune tracking refs no longer on the remote
    #[arg(long = "prune", action = clap::ArgAction::SetTrue)]
    pub prune: bool,
}

#[derive(Debug, clap::Parser)]
pub struct PushArgs {
    /// Remote name (or URL) to push to
    #[arg(short = 'r', long = "remote", default_value = "origin")]
    pub remote: String,

    /// One or more refspecs to push (e.g. main, HEAD:refs/heads/main)
    #[arg(short = 'R', long = "ref")]
    pub refspecs: Vec<String>,

    /// Force update (non-fast-forward)
    #[arg(short = 'f', long = "force", action = clap::ArgAction::SetTrue)]
    pub force: bool,

    /// Atomic push (all-or-nothing if server supports it)
    #[arg(long = "atomic", action = clap::ArgAction::SetTrue)]
    pub atomic: bool,

    /// Dry run (don't actually update remote)
    #[arg(long = "dry-run", action = clap::ArgAction::SetTrue)]
    pub dry_run: bool,

    /// Use split mode for pushing (defaults to normal mode)
    #[arg(long = "split", action = clap::ArgAction::SetTrue)]
    pub split: bool,

    /// Use stack mode for pushing (defaults to normal mode)
    #[arg(long = "stack", action = clap::ArgAction::SetTrue)]
    pub stack: bool,
}

#[derive(Debug, clap::Parser)]
pub struct RemoteArgs {
    /// Remote subcommand
    #[command(subcommand)]
    pub command: RemoteCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum RemoteCommand {
    /// Add a remote with optional projection/filtering
    Add(RemoteAddArgs),
}

#[derive(Debug, clap::Parser)]
pub struct RemoteAddArgs {
    /// Remote name
    #[arg()]
    pub name: String,

    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Workspace/projection identifier or path to spec
    #[arg()]
    pub filter: String,

    /// Keep trivial merges (don't append :prune=trivial-merge to filters)
    #[arg(long = "keep-trivial-merges")]
    pub keep_trivial_merges: bool,
}

#[derive(Debug, clap::Parser)]
pub struct FilterArgs {
    /// Remote name to apply filtering to
    #[arg()]
    pub remote: String,
}

#[derive(Debug, clap::Parser)]
#[cfg(feature = "incubating")]
pub struct LinkArgs {
    /// Link subcommand
    #[command(subcommand)]
    pub command: LinkCommand,
}

#[cfg(feature = "incubating")]
#[derive(Debug, clap::Subcommand)]
pub enum LinkCommand {
    /// Add a link with optional filter and target branch
    Add(LinkAddArgs),
    /// Fetch from existing link files
    Fetch(LinkFetchArgs),
}

#[cfg(feature = "incubating")]
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

#[cfg(feature = "incubating")]
#[derive(Debug, clap::Parser)]
pub struct LinkFetchArgs {
    /// Optional path to specific .link.josh file (if not provided, fetches all)
    #[arg()]
    pub path: Option<String>,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let result = run_command(&cli);

    if let Err(e) = result {
        eprintln!("Error: {e}");

        for e in e.chain() {
            eprintln!("{e}");
        }

        std::process::exit(1);
    }
}

fn run_command(cli: &Cli) -> anyhow::Result<()> {
    // For clone, do the initial repo setup before creating transaction
    let repo_path = if let Command::Clone(args) = &cli.command {
        // For clone, we're not in a git repo initially, so clone first and use that path
        clone_repo(args)?
    } else {
        // For other commands, we need to be in a git repo
        let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
        repo.path().parent().unwrap().to_path_buf()
    };

    josh_core::cache::sled_load(&repo_path.join(".git")).context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default()),
        // FIXME: NotesCacheBackend seems to have perf issues, so disable it for now
        //.with_backend(
        //    josh_core::cache::NotesCacheBackend::new(&repo_path)
        //        ?
        //        .context("Failed to create NotesCacheBackend")?,
        //),
    );

    // Create transaction using the known repo path
    let transaction = josh_core::cache::TransactionContext::new(&repo_path, cache.clone())
        .open(None)
        .context("Failed TransactionContext::open")?;

    match &cli.command {
        Command::Clone(args) => handle_clone(args, &transaction),
        Command::Fetch(args) => handle_fetch(args, &transaction),
        Command::Pull(args) => handle_pull(args, &transaction),
        Command::Push(args) => handle_push(args, &transaction),
        Command::Remote(args) => handle_remote(args, &transaction),
        Command::Filter(args) => handle_filter(args, &transaction),
        #[cfg(feature = "incubating")]
        Command::Link(args) => handle_link(args, &transaction),
    }
}

/// Apply josh filtering to all remote refs and update local refs
fn apply_josh_filtering(
    transaction: &josh_core::cache::Transaction,
    repo_path: &std::path::Path,
    filter: &str,
    remote_name: &str,
) -> anyhow::Result<()> {
    // Use josh API directly instead of calling josh-filter binary
    let filterobj = josh_core::filter::parse(filter).context("Failed to parse filter")?;

    let repo = transaction.repo();

    // Get all remote refs from refs/josh/remotes/{remote_name}/*
    let mut input_refs = Vec::new();
    let josh_remotes = repo.references_glob(&format!("refs/josh/remotes/{}/*", remote_name))?;

    for reference in josh_remotes {
        let reference = reference?;
        if let Some(target) = reference.target() {
            let ref_name = reference.name().unwrap().to_string();
            input_refs.push((ref_name, target));
        }
    }

    if input_refs.is_empty() {
        return Err(anyhow!("No remote references found"));
    }

    // Apply the filter to all remote refs
    let (updated_refs, errors) = josh_core::filter_refs(&transaction, filterobj, &input_refs);

    // Check for errors
    if let Some(error) = errors.into_iter().next() {
        return Err(anyhow!("josh filter error: {}", error.1));
    }

    // Second pass: create all references
    for (original_ref, filtered_oid) in updated_refs {
        // Check if the filtered result is empty (zero OID indicates empty result)
        if filtered_oid == git2::Oid::zero() {
            // Skip creating references for empty filtered results
            continue;
        }

        // Extract branch name from refs/josh/remotes/{remote_name}/branch_name
        let branch_name = original_ref
            .strip_prefix(&format!("refs/josh/remotes/{}/", remote_name))
            .context("Invalid josh remote reference")?;

        // Create filtered reference in josh/filtered namespace
        let filtered_ref = format!(
            "refs/namespaces/josh-{}/refs/heads/{}",
            remote_name, branch_name
        );

        repo.reference(&filtered_ref, filtered_oid, true, "josh filter")
            .context("failed to create filtered reference")?;
    }

    spawn_git_command(repo_path, &["fetch", remote_name], &[])
        .context("failed to fetch filtered refs")?;

    Ok(())
}

fn to_absolute_remote_url(url: &str) -> anyhow::Result<String> {
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ssh://")
        || url.starts_with("file://")
    {
        Ok(url.to_owned())
    } else {
        // For local paths, make them absolute
        let path = std::fs::canonicalize(url)
            .with_context(|| format!("Failed to resolve path {}", url))?
            .display()
            .to_string();

        Ok(format!("file://{}", path))
    }
}

/// Read remote configuration from .git/josh/remotes/<name>.josh file
/// Falls back to legacy git config josh-remote section if file doesn't exist
fn read_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
) -> anyhow::Result<(String, String, josh_core::filter::Filter)> {
    let remotes_dir = repo_path.join(".git").join("josh").join("remotes");
    let remote_file = remotes_dir.join(format!("{}.josh", remote_name));

    // Try to read from the new file format first
    match std::fs::read_to_string(&remote_file) {
        Ok(content) => {
            // Parse the filter from the file
            let filter = josh_core::filter::parse(&content).with_context(|| {
                format!("Failed to parse filter from {}", remote_file.display())
            })?;

            // Extract metadata
            let url = filter
                .get_meta("url")
                .ok_or_else(|| anyhow!("Missing 'url' metadata in remote config"))?;
            let fetch = filter
                .get_meta("fetch")
                .ok_or_else(|| anyhow!("Missing 'fetch' metadata in remote config"))?;

            return Ok((url, fetch, filter));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist, try legacy git config
            let repo = git2::Repository::open(repo_path)
                .context("Failed to open repository for legacy config migration")?;
            let config = repo.config().context("Failed to get git config")?;

            // Try to read from legacy josh-remote config
            let url = match config.get_string(&format!("josh-remote.{}.url", remote_name)) {
                Ok(url) => url,
                Err(_) => {
                    return Err(anyhow!(
                        "Remote '{}' not found in new format (.git/josh/remotes/{}.josh) or legacy git config (josh-remote.{})",
                        remote_name,
                        remote_name,
                        remote_name
                    ));
                }
            };

            let filter_str = config
                .get_string(&format!("josh-remote.{}.filter", remote_name))
                .with_context(|| {
                    format!("Legacy config missing filter for remote '{}'", remote_name)
                })?;

            let fetch = config
                .get_string(&format!("josh-remote.{}.fetch", remote_name))
                .with_context(|| {
                    format!("Legacy config missing fetch for remote '{}'", remote_name)
                })?;

            // Migrate to new format by writing the file
            write_remote_config(repo_path, remote_name, &url, &filter_str, &fetch)
                .context("Failed to migrate legacy config to new format")?;

            // Parse the filter to return
            let filter_obj = josh_core::filter::parse(&filter_str)
                .with_context(|| format!("Failed to parse filter '{}'", filter_str))?;

            let filter_with_meta = filter_obj.with_meta("url", &url).with_meta("fetch", &fetch);

            log::info!(
                "Migrated remote '{}' from legacy git config to new file format",
                remote_name
            );

            Ok((url, fetch, filter_with_meta))
        }
        Err(e) => {
            return Err(anyhow!(
                "Failed to read remote config file: {}: {}",
                remote_file.display(),
                e
            ));
        }
    }
}

/// Write remote configuration to .git/josh/remotes/<name>.josh file
fn write_remote_config(
    repo_path: &std::path::Path,
    remote_name: &str,
    url: &str,
    filter: &str,
    fetch: &str,
) -> anyhow::Result<()> {
    let remotes_dir = repo_path.join(".git").join("josh").join("remotes");

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
    let filter_with_meta = filter_obj.with_meta("url", url).with_meta("fetch", fetch);

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

/// Initial clone setup: create directory, init repo, add remote (no transaction needed)
fn clone_repo(args: &CloneArgs) -> anyhow::Result<std::path::PathBuf> {
    // Use the provided output directory
    let output_dir = args.out.clone();

    // Create the output directory first
    std::fs::create_dir_all(&output_dir)?;

    // Initialize a new git repository inside the directory using git2
    git2::Repository::init(&output_dir).context("Failed to initialize git repository")?;

    // Use handle_remote_add to add the remote with the filter
    let remote_add_args = RemoteAddArgs {
        name: "origin".to_string(),
        url: to_absolute_remote_url(&args.url)?,
        filter: args.filter.clone(),
        keep_trivial_merges: args.keep_trivial_merges,
    };

    handle_remote_add_repo(&remote_add_args, &output_dir)?;

    Ok(output_dir)
}

fn handle_clone(
    args: &CloneArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Create FetchArgs from CloneArgs
    let fetch_args = FetchArgs {
        remote: "origin".to_string(),
        rref: args.branch.clone(),
        prune: false,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch_repo(&fetch_args, transaction)?;

    // Get the default branch name from the remote HEAD symref
    let default_branch = if args.branch == "HEAD" {
        // Read the remote HEAD symref to get the default branch
        let head_ref = "refs/remotes/origin/HEAD".to_string();

        let head_reference = repo
            .find_reference(&head_ref)
            .with_context(|| format!("Failed to find remote HEAD reference {}", head_ref))?;

        let symref_target = head_reference
            .symbolic_target()
            .context("Remote HEAD reference is not a symbolic reference")?;

        // Extract branch name from symref target (e.g., "refs/remotes/origin/master" -> "master")
        let branch_name = symref_target
            .strip_prefix("refs/remotes/origin/")
            .with_context(|| format!("Invalid symref target format: {}", symref_target))?;

        branch_name.to_string()
    } else {
        args.branch.clone()
    };

    spawn_git_command(
        repo.path(),
        &[
            "checkout",
            "-b",
            &default_branch,
            &format!("origin/{}", default_branch),
        ],
        &[],
    )
    .with_context(|| format!("Failed to checkout branch {}", default_branch))?;

    // Set up upstream tracking for the branch
    spawn_git_command(
        repo.path(),
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", default_branch),
            &default_branch,
        ],
        &[],
    )
    .with_context(|| format!("Failed to set upstream for branch {}", default_branch))?;

    let output_dir = normalize_repo_path(repo.path());
    let output_dir = output_dir.display().to_string();

    let output_dir = if let Ok(testtmp) = std::env::var("TESTTMP") {
        output_dir.replace(&testtmp, "${TESTTMP}")
    } else {
        output_dir.to_string()
    };

    println!("Cloned repository to: {}", output_dir);
    Ok(())
}

fn handle_pull(args: &PullArgs, transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Create FetchArgs from PullArgs
    let fetch_args = FetchArgs {
        remote: args.remote.clone(),
        rref: args.rref.clone(),
        prune: args.prune,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch_repo(&fetch_args, transaction)?;

    // Now use actual git pull to integrate the changes
    let mut git_args = vec!["pull"];

    if args.rebase {
        git_args.push("--rebase");
    }

    if args.autostash {
        git_args.push("--autostash");
    }

    git_args.push(&args.remote);

    spawn_git_command(repo.path(), &git_args, &[]).context("git pull failed")?;

    eprintln!("Pulled from remote: {}", args.remote);

    Ok(())
}

fn handle_fetch(
    args: &FetchArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    handle_fetch_repo(args, transaction)
}

fn try_parse_symref(remote: &str, output: &str) -> Option<(String, String)> {
    let line = output.lines().next()?;
    let symref_part = line.split('\t').next()?;

    let default_branch = symref_part.strip_prefix("ref: refs/heads/")?;
    let default_branch_ref = format!("refs/remotes/{}/{}", remote, default_branch);

    Some((default_branch.to_string(), default_branch_ref))
}

fn handle_fetch_repo(
    args: &FetchArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = repo.path().parent().unwrap();

    // Read the remote configuration from .git/josh/remotes/<name>.josh
    let (remote_url, refspec, _filter) = read_remote_config(repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    // First, fetch unfiltered refs to refs/josh/remotes/*
    spawn_git_command(repo.path(), &["fetch", &remote_url, &refspec], &[])
        .context("git fetch to josh/remotes failed")?;

    // Set up remote HEAD reference using git ls-remote
    // This is the proper way to get the default branch from the remote
    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);

    // Use git ls-remote --symref to get the default branch
    // Parse the output to get the default branch name
    // Output format: "ref: refs/heads/main\t<commit-hash>"
    let output = std::process::Command::new("git")
        .args(["ls-remote", "--symref", &remote_url, "HEAD"])
        .current_dir(repo.path().parent().unwrap())
        .output()?;

    if output.status.success() {
        let output = String::from_utf8(output.stdout)?;

        if let Some((default_branch, default_branch_ref)) = try_parse_symref(&args.remote, &output)
        {
            repo.reference_symbolic(&head_ref, &default_branch_ref, true, "josh remote HEAD")?;

            repo.reference_symbolic(
                &format!("refs/namespaces/josh-{}/{}", args.remote, "HEAD"),
                &format!("refs/heads/{}", default_branch),
                true,
                "josh remote HEAD",
            )?;
        }
    }

    // Apply josh filtering using handle_filter_internal (without messages)
    let filter_args = FilterArgs {
        remote: args.remote.clone(),
    };

    handle_filter_repo(&filter_args, false, transaction)?;

    // Note: fetch doesn't checkout, it just updates the refs
    eprintln!("Fetched from remote: {}", args.remote);

    Ok(())
}

fn handle_push(args: &PushArgs, transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    // Read remote configuration from .git/josh/remotes/<name>.josh
    let repo = transaction.repo();
    let repo_path = repo.path().parent().unwrap();

    let (remote_url, _refspec, filter_with_meta) = read_remote_config(repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    // Get the wrapped filter (peel away metadata)
    let filter = filter_with_meta.peel();

    // Get git config for user email
    let config = repo.config().context("Failed to get git config")?;

    // If no refspecs provided, push the current branch
    let refspecs = if args.refspecs.is_empty() {
        // Get the current branch name
        let head = repo.head().context("Failed to get HEAD")?;

        let current_branch = head
            .shorthand()
            .context("Failed to get current branch name")?;

        vec![current_branch.to_string()]
    } else {
        args.refspecs.clone()
    };

    // For each refspec, we need to:
    // 1. Get the current commit of the local ref
    // 2. Use Josh API to unapply the filter
    // 3. Push the unfiltered result to the remote

    for refspec in &refspecs {
        let (local_ref, remote_ref) = if let Some(colon_pos) = refspec.find(':') {
            let local = &refspec[..colon_pos];
            let remote = &refspec[colon_pos + 1..];
            (local.to_string(), remote.to_string())
        } else {
            // If no colon, push local ref to remote with same name
            (refspec.clone(), refspec.clone())
        };
        let remote_ref = remote_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&remote_ref);

        // Get the current commit of the local ref
        let local_commit = repo
            .resolve_reference_from_short_name(&local_ref)
            .with_context(|| format!("Failed to resolve local ref '{}'", local_ref))?
            .target()
            .context("Failed to get target of local ref")?;

        // Get the original target (the base commit that was filtered)
        // We need to find the original commit in the unfiltered repository
        // that corresponds to the current filtered commit
        // Use josh/remotes references which contain the unfiltered commits
        let josh_remote_ref = format!("refs/josh/remotes/{}/{}", args.remote, remote_ref);
        let original_target = if let Ok(remote_reference) = repo.find_reference(&josh_remote_ref) {
            // If we have a josh remote reference, use its target (this is the unfiltered commit)
            remote_reference.target().unwrap_or(git2::Oid::zero())
        } else {
            // If no josh remote reference, this is a new push
            git2::Oid::zero()
        };

        // Get the old filtered oid by applying the filter to the original remote ref
        // before we push to the namespace
        let josh_remote_ref = format!("refs/josh/remotes/{}/{}", args.remote, remote_ref);
        let old_filtered_oid =
            if let Ok(josh_remote_reference) = repo.find_reference(&josh_remote_ref) {
                let josh_remote_oid = josh_remote_reference.target().unwrap_or(git2::Oid::zero());

                // Apply the filter to the josh remote ref to get the old filtered oid
                let (filtered_oids, errors) = josh_core::filter_refs(
                    transaction,
                    filter,
                    &[(josh_remote_ref.clone(), josh_remote_oid)],
                );

                // Check for errors
                if let Some(error) = errors.into_iter().next() {
                    return Err(anyhow!("josh filter error: {}", error.1));
                }

                if let Some((_, filtered_oid)) = filtered_oids.first() {
                    *filtered_oid
                } else {
                    git2::Oid::zero()
                }
            } else {
                // If no josh remote reference, this is a new push
                git2::Oid::zero()
            };

        log::debug!("old_filtered_oid: {:?}", old_filtered_oid);
        log::debug!("original_target: {:?}", original_target);

        // Set push mode based on the metadata
        let push_mode = if args.split {
            PushMode::Split
        } else if args.stack {
            PushMode::Stack
        } else {
            PushMode::Normal
        };

        // Get author email from git config
        let author = config.get_string("user.email").unwrap_or_default();

        let mut changes: Option<Vec<josh_core::Change>> =
            if push_mode == PushMode::Stack || push_mode == PushMode::Split {
                Some(vec![])
            } else {
                None
            };

        // Use Josh API to unapply the filter
        let unfiltered_oid = josh_core::history::unapply_filter(
            transaction,
            filter,
            original_target,
            old_filtered_oid,
            local_commit,
            josh_core::history::OrphansMode::Keep,
            None,         // reparent_orphans
            &mut changes, // change_ids
        )
        .context("Failed to unapply filter")?;

        // Define variables needed for build_to_push
        let baseref = remote_ref;
        let oid_to_push = unfiltered_oid;
        let old = original_target;

        log::debug!("unfiltered_oid: {:?}", unfiltered_oid);

        let to_push = build_to_push(
            transaction.repo(),
            changes,
            push_mode,
            &baseref,
            &author,
            &remote_ref,
            oid_to_push,
            old,
        )
        .context("Failed to build to push")?;

        log::debug!("to_push: {:?}", to_push);

        // Process each entry in to_push (similar to josh-proxy)
        for (refname, oid, _) in to_push {
            // Build git push command
            let mut git_push_args = vec!["push"];

            if args.force || push_mode == PushMode::Split {
                git_push_args.push("--force");
            }

            if args.atomic {
                git_push_args.push("--atomic");
            }

            if args.dry_run {
                git_push_args.push("--dry-run");
            }

            // Determine the target remote URL
            let target_remote = remote_url.clone();

            // Create refspec: oid:refname
            let push_refspec = format!("{}:{}", oid, refname);

            git_push_args.push(&target_remote);
            git_push_args.push(&push_refspec);

            // Use direct spawn so users can see git push progress
            spawn_git_command(
                repo.path(),
                &git_push_args, // Skip "git" since spawn_git_command adds it
                &[],
            )
            .context("git push failed")?;

            eprintln!("Pushed {} to {}/{}", oid, args.remote, refname);
        }
    }

    Ok(())
}

#[cfg(feature = "incubating")]
fn handle_link(args: &LinkArgs, transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args, transaction),
        LinkCommand::Fetch(fetch_args) => handle_link_fetch(fetch_args, transaction),
    }
}

#[cfg(feature = "incubating")]
fn handle_link_add(
    args: &LinkAddArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    use josh_link::make_signature;

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

#[cfg(feature = "incubating")]
fn handle_link_fetch(
    args: &LinkFetchArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    use josh_link::make_signature;

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

fn handle_remote(
    args: &RemoteArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    match &args.command {
        RemoteCommand::Add(add_args) => {
            let repo_path = transaction.repo().path().parent().unwrap();
            handle_remote_add_repo(add_args, repo_path)
        }
    }
}

fn handle_remote_add_repo(args: &RemoteAddArgs, repo_path: &std::path::Path) -> anyhow::Result<()> {
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;
    let workdir = normalize_repo_path(repo_path);

    // Store the remote information in .git/josh/remotes/<name>.josh file
    let remote_url = to_absolute_remote_url(&args.url)?;

    // Store the filter in git config per remote
    let filter_to_store = args.filter.clone();

    // Store refspec (for unfiltered refs)
    let refspec = format!("+refs/heads/*:refs/josh/remotes/{}/*", args.name);

    // Write remote config to .git/josh/remotes/<name>.josh
    write_remote_config(
        repo_path,
        &args.name,
        &remote_url,
        &filter_to_store,
        &refspec,
    )
    .context("Failed to write remote config file")?;

    // Set up a git remote that points to "." with a refspec to fetch filtered refs
    // Add remote pointing to current directory
    let repo_remote = to_absolute_remote_url(&workdir.display().to_string())?;
    spawn_git_command(
        repo.path(),
        &["remote", "add", &args.name, &repo_remote],
        &[],
    )
    .context("Failed to add git remote")?;

    // Set up namespace configuration for the remote
    let namespace = format!("josh-{}", args.name);
    let uploadpack_cmd = format!("env GIT_NAMESPACE={} git upload-pack", namespace);

    spawn_git_command(
        repo.path(),
        &[
            "config",
            &format!("remote.{}.uploadpack", args.name),
            &uploadpack_cmd,
        ],
        &[],
    )
    .context("Failed to set remote uploadpack")?;

    eprintln!(
        "Added remote '{}' with filter '{}'",
        args.name, filter_to_store
    );

    Ok(())
}

/// Handle the `josh filter` command - apply filtering to existing refs without fetching
fn handle_filter(
    args: &FilterArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    handle_filter_repo(args, true, transaction)
}

/// Internal filter function that can be called from other handlers
fn handle_filter_repo(
    args: &FilterArgs,
    print_messages: bool,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = repo.path().parent().unwrap();

    // Read the remote configuration from .git/josh/remotes/<name>.josh
    let (_url, _refspec, filter_with_meta) = read_remote_config(repo_path, &args.remote)
        .with_context(|| format!("Failed to read remote config for '{}'", args.remote))?;

    // Get the wrapped filter (peel away metadata)
    let filter = filter_with_meta.peel();

    // Serialize the filter for display/logging
    let filter_str = josh_core::filter::spec(filter);

    if print_messages {
        println!(
            "Applying filter '{}' to remote '{}'",
            filter_str, args.remote
        );
    }

    // Apply josh filtering (this is the same as in handle_fetch but without the git fetch step)
    apply_josh_filtering(transaction, repo_path, &filter_str, &args.remote)?;

    if print_messages {
        println!("Applied filter to remote: {}", args.remote);
    }

    Ok(())
}
