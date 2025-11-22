#![warn(unused_extern_crates)]

use anyhow::Context;
use clap::Parser;
use josh_core::changes::{PushMode, build_to_push};
use josh_core::shell::Shell;

use std::io::IsTerminal;
use std::process::{Command as ProcessCommand, Stdio};

/// Helper function to convert josh_core::JoshError to anyhow::Error
fn from_josh_err(err: josh_core::JoshError) -> anyhow::Error {
    anyhow::anyhow!("{}", err.0)
}

/// Spawn a git command directly to the terminal so users can see progress
/// Falls back to captured output if not in a TTY environment
fn spawn_git_command(
    cwd: &std::path::Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> anyhow::Result<()> {
    log::debug!("spawn_git_command: {:?}", args);

    let mut command = ProcessCommand::new("git");
    command.current_dir(cwd).args(args);

    for (key, value) in env {
        command.env(key, value);
    }

    // Check if we're in a TTY environment
    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    let status = if is_tty {
        // In TTY: inherit stdio so users can see progress
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        command.status()?.code()
    } else {
        // Not in TTY: capture output and print stderr (for tests, CI, etc.)
        // Use the same approach as josh_core::shell::Shell for consistency
        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("failed to execute git command")?;

        // Print stderr if there's any output
        if !output.stderr.is_empty() {
            let output_str = String::from_utf8_lossy(&output.stderr);
            let output_str = if let Ok(testtmp) = std::env::var("TESTTMP") {
                output_str.replace(&testtmp, "${TESTTMP}")
            } else {
                output_str.to_string()
            };

            eprintln!("{}", output_str);
        }

        output.status.code()
    };

    match status.unwrap_or(1) {
        0 => Ok(()),
        code => {
            let command = args.join(" ");
            Err(anyhow::anyhow!(
                "Command exited with code {}: git {}",
                code,
                command
            ))
        }
    }
}

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
    /// Optional path to specific .josh-link.toml file (if not provided, fetches all)
    #[arg()]
    pub path: Option<String>,
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Clone(args) => handle_clone(args),
        Command::Fetch(args) => handle_fetch(args),
        Command::Pull(args) => handle_pull(args),
        Command::Push(args) => handle_push(args),
        Command::Remote(args) => handle_remote(args),
        Command::Filter(args) => handle_filter(args),
        #[cfg(feature = "incubating")]
        Command::Link(args) => handle_link(args),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");

        for e in e.chain() {
            eprintln!("{e}");
        }

        std::process::exit(1);
    }
}

/// Apply josh filtering to all remote refs and update local refs
fn apply_josh_filtering(
    repo_path: &std::path::Path,
    filter: &str,
    remote_name: &str,
) -> anyhow::Result<()> {
    // Use josh API directly instead of calling josh-filter binary
    let filterobj = josh_core::filter::parse(filter)
        .map_err(from_josh_err)
        .context("Failed to parse filter")?;

    josh_core::cache_sled::sled_load(&repo_path.join(".git"))
        .map_err(from_josh_err)
        .context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache_stack::CacheStack::new()
            .with_backend(josh_core::cache_sled::SledCacheBackend::default())
            .with_backend(
                josh_core::cache_notes::NotesCacheBackend::new(repo_path)
                    .map_err(from_josh_err)
                    .context("Failed to create NotesCacheBackend")?,
            ),
    );

    // Open Josh transaction
    let transaction = josh_core::cache::TransactionContext::new(repo_path, cache.clone())
        .open(None)
        .map_err(from_josh_err)?;

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
        return Err(anyhow::anyhow!("No remote references found"));
    }

    // Apply the filter to all remote refs
    let (updated_refs, errors) = josh_core::filter_refs(
        &transaction,
        filterobj,
        &input_refs,
        josh_core::filter::empty(),
    );

    // Check for errors
    for error in errors {
        return Err(anyhow::anyhow!("josh filter error: {}", error.1.0));
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
        let path = std::fs::canonicalize(&url)
            .with_context(|| format!("Failed to resolve path {}", url))?
            .display()
            .to_string();

        Ok(format!("file://{}", path))
    }
}

fn handle_clone(args: &CloneArgs) -> anyhow::Result<()> {
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

    // Create FetchArgs from CloneArgs
    let fetch_args = FetchArgs {
        remote: "origin".to_string(),
        rref: args.branch.clone(),
        prune: false,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch_repo(&fetch_args, &output_dir)?;

    // Get the default branch name from the remote HEAD symref
    let default_branch = if args.branch == "HEAD" {
        // Read the remote HEAD symref to get the default branch
        let head_ref = "refs/remotes/origin/HEAD".to_string();
        let repo = git2::Repository::open(&output_dir).context("Failed to open repository")?;

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
        &output_dir,
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
        &output_dir,
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", default_branch),
            &default_branch,
        ],
        &[],
    )
    .with_context(|| format!("Failed to set upstream for branch {}", default_branch))?;

    let output_str = output_dir.display().to_string();
    let output_str = if let Ok(testtmp) = std::env::var("TESTTMP") {
        output_str.replace(&testtmp, "${TESTTMP}")
    } else {
        output_str.to_string()
    };
    println!("Cloned repository to: {}", output_str);
    Ok(())
}

fn handle_pull(args: &PullArgs) -> anyhow::Result<()> {
    // Check if we're in a git repository
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
    let repo_path = repo.path().parent().unwrap().to_path_buf();

    // Create FetchArgs from PullArgs
    let fetch_args = FetchArgs {
        remote: args.remote.clone(),
        rref: args.rref.clone(),
        prune: args.prune,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch_repo(&fetch_args, &repo_path)?;

    // Get current working directory for shell commands
    let current_dir = std::env::current_dir()?;
    let repo_shell = Shell {
        cwd: current_dir.clone(),
    };

    // Now use actual git pull to integrate the changes
    let mut git_args = vec!["pull"];

    if args.rebase {
        git_args.push("--rebase");
    }

    if args.autostash {
        git_args.push("--autostash");
    }

    git_args.push(&args.remote);

    spawn_git_command(repo_shell.cwd.as_path(), &git_args, &[]).context("git pull failed")?;

    eprintln!("Pulled from remote: {}", args.remote);

    Ok(())
}

fn handle_fetch(args: &FetchArgs) -> anyhow::Result<()> {
    // Check if we're in a git repository
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
    let repo_path = repo.path().parent().unwrap().to_path_buf();

    handle_fetch_repo(args, &repo_path)
}

fn try_parse_symref(remote: &str, output: &str) -> Option<(String, String)> {
    let line = output.lines().next()?;
    let symref_part = line.split('\t').next()?;

    let default_branch = symref_part.strip_prefix("ref: refs/heads/")?;
    let default_branch_ref = format!("refs/remotes/{}/{}", remote, default_branch);

    Some((default_branch.to_string(), default_branch_ref))
}

fn handle_fetch_repo(args: &FetchArgs, repo_path: &std::path::Path) -> anyhow::Result<()> {
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;

    // Read the remote URL from josh-remote config
    let config = repo.config().context("Failed to get git config")?;

    let remote_url = config
        .get_string(&format!("josh-remote.{}.url", args.remote))
        .with_context(|| format!("Failed to get remote URL for '{}'", args.remote))?;

    let refspec = config
        .get_string(&format!("josh-remote.{}.fetch", args.remote))
        .with_context(|| format!("Failed to get refspec for '{}'", args.remote))?;

    // First, fetch unfiltered refs to refs/josh/remotes/*
    spawn_git_command(repo_path, &["fetch", &remote_url, &refspec], &[])
        .context("git fetch to josh/remotes failed")?;

    // Set up remote HEAD reference using git ls-remote
    // This is the proper way to get the default branch from the remote
    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);

    // Use git ls-remote --symref to get the default branch
    // Parse the output to get the default branch name
    // Output format: "ref: refs/heads/main\t<commit-hash>"
    let output = std::process::Command::new("git")
        .args(&["ls-remote", "--symref", &remote_url, "HEAD"])
        .current_dir(repo_path)
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

    handle_filter_repo(&filter_args, repo_path, false)?;

    // Note: fetch doesn't checkout, it just updates the refs
    eprintln!("Fetched from remote: {}", args.remote);

    Ok(())
}

fn handle_push(args: &PushArgs) -> anyhow::Result<()> {
    // Read filter from git config for the specific remote
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
    let repo_path = repo.path().parent().unwrap();

    let config = repo.config().context("Failed to get git config")?;

    // Step 2: Apply reverse filtering and push to actual remote
    let filter_str = config
        .get_string(&format!("josh-remote.{}.filter", args.remote))
        .context("Failed to read filter from git config")?;

    // Parse the filter using Josh API
    let filter = josh_core::filter::parse(&filter_str)
        .map_err(from_josh_err)
        .context("Failed to parse filter")?;

    josh_core::cache_sled::sled_load(repo_path)
        .map_err(from_josh_err)
        .context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache_stack::CacheStack::new()
            .with_backend(josh_core::cache_sled::SledCacheBackend::default())
            .with_backend(
                josh_core::cache_notes::NotesCacheBackend::new(repo_path)
                    .map_err(from_josh_err)
                    .context("Failed to create NotesCacheBackend")?,
            ),
    );

    // Open Josh transaction
    let transaction = josh_core::cache::TransactionContext::from_env(cache.clone())
        .map_err(from_josh_err)
        .context("Failed TransactionContext::from_env")?
        .open(None)
        .map_err(from_josh_err)
        .context("Failed TransactionContext::open")?;

    // Get the remote URL from josh-remote config
    let remote_url = config
        .get_string(&format!("josh-remote.{}.url", args.remote))
        .with_context(|| format!("Failed to get remote URL for '{}'", args.remote))?;

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
                    &transaction,
                    filter,
                    &[(josh_remote_ref.clone(), josh_remote_oid)],
                    josh_core::filter::empty(),
                );

                // Check for errors
                for error in errors {
                    return Err(anyhow::anyhow!("josh filter error: {}", error.1.0).into());
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

        // Set push mode based on the flags
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
            &transaction,
            filter,
            original_target,
            old_filtered_oid,
            local_commit,
            josh_core::history::OrphansMode::Keep,
            None,         // reparent_orphans
            &mut changes, // change_ids
        )
        .map_err(from_josh_err)
        .context("Failed to unapply filter")?;

        // Define variables needed for build_to_push
        let baseref = remote_ref.clone();
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
        .map_err(from_josh_err)
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
                repo_path,
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
fn handle_link(args: &LinkArgs) -> anyhow::Result<()> {
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args),
        LinkCommand::Fetch(fetch_args) => handle_link_fetch(fetch_args),
    }
}

#[cfg(feature = "incubating")]
fn handle_link_add(args: &LinkAddArgs) -> anyhow::Result<()> {
    use josh_core::filter::tree;
    use josh_core::{JoshLinkFile, Oid};

    // Check if we're in a git repository
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;

    // Validate the path (should not be empty and should be a valid path)
    if args.path.is_empty() {
        return Err(anyhow::anyhow!("Path cannot be empty"));
    }

    // Normalize the path by removing leading and trailing slashes
    let normalized_path = args.path.trim_matches('/').to_string();

    // Get the filter (default to ":/" if not provided)
    let filter = args.filter.as_deref().unwrap_or(":/");

    // Get the target branch (default to "HEAD" if not provided)
    let target = args.target.as_deref().unwrap_or("HEAD");

    // Parse the filter
    let filter_obj = josh_core::filter::parse(filter)
        .map_err(from_josh_err)
        .with_context(|| format!("Failed to parse filter '{}'", filter))?;

    // Use git fetch shell command
    let output = std::process::Command::new("git")
        .args(&["fetch", &args.url, &target])
        .output()
        .context("Failed to execute git fetch")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
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
    let actual_commit_sha = fetch_commit.id();

    // Get the current HEAD commit
    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    // Create the JoshLinkFile with the actual commit SHA
    let link_file = JoshLinkFile {
        remote: args.url.clone(),
        branch: target.to_string(),
        filter: filter_obj,
        commit: Oid::from(actual_commit_sha),
    };

    // Create the .josh-link.toml content
    let link_content = toml::to_string(&link_file).context("Failed to serialize link file")?;

    // Create the blob for the .josh-link.toml file
    let link_blob = repo
        .blob(link_content.as_bytes())
        .context("Failed to create blob")?;

    // Create the path for the .josh-link.toml file
    let link_path = std::path::Path::new(&normalized_path).join(".josh-link.toml");

    // Insert the .josh-link.toml file into the tree
    let new_tree = tree::insert(&repo, &head_tree, &link_path, link_blob, 0o0100644)
        .map_err(from_josh_err)
        .context("Failed to insert link file into tree")?;

    // Create a new commit with the updated tree
    let signature = if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        git2::Signature::new(
            "JOSH",
            "josh@josh-project.dev",
            &git2::Time::new(time.parse().context("Failed to parse JOSH_COMMIT_TIME")?, 0),
        )
        .context("Failed to create signature")?
    } else {
        repo.signature().context("Failed to get signature")?
    };

    let new_commit = repo
        .commit(
            None, // Don't update any reference
            &signature,
            &signature,
            &format!("Add link: {}", normalized_path),
            &new_tree,
            &[&head_commit],
        )
        .context("Failed to create commit")?;

    // Apply the :link filter to the new commit
    let link_filter = josh_core::filter::parse(":link=snapshot")
        .map_err(from_josh_err)
        .context("Failed to parse :link filter")?;

    // Load the cache and create transaction
    let repo_path = repo.path().parent().unwrap();

    josh_core::cache_sled::sled_load(&repo_path).unwrap();
    let cache = std::sync::Arc::new(
        josh_core::cache_stack::CacheStack::new()
            .with_backend(josh_core::cache_sled::SledCacheBackend::default())
            .with_backend(
                josh_core::cache_notes::NotesCacheBackend::new(&repo_path)
                    .map_err(from_josh_err)
                    .context("Failed to create NotesCacheBackend")?,
            ),
    );

    // Open Josh transaction
    let transaction = josh_core::cache::TransactionContext::from_env(cache.clone())
        .map_err(from_josh_err)
        .context("Failed TransactionContext::from_env")?
        .open(None)
        .map_err(from_josh_err)
        .context("Failed TransactionContext::open")?;

    let filtered_commit = josh_core::filter_commit(
        &transaction,
        link_filter,
        new_commit,
        josh_core::filter::empty(),
    )
    .map_err(from_josh_err)
    .context("Failed to apply :link filter")?;

    // Create the fixed branch name
    let branch_name = "refs/heads/josh-link";

    // Create or update the branch reference
    repo.reference(branch_name, filtered_commit, true, "josh link add")
        .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

    println!(
        "Added link '{}' with URL '{}', filter '{}', and target '{}'",
        normalized_path, args.url, filter, target
    );
    println!("Created branch: {}", branch_name);

    Ok(())
}

#[cfg(feature = "incubating")]
fn handle_link_fetch(args: &LinkFetchArgs) -> anyhow::Result<()> {
    use josh_core::filter::tree;
    use josh_core::{JoshLinkFile, Oid};

    // Check if we're in a git repository
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;

    // Get the current HEAD commit
    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to get HEAD commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files = if let Some(path) = &args.path {
        // Single path specified - find the .josh-link.toml file at that path
        let link_path = std::path::Path::new(path).join(".josh-link.toml");
        let link_entry = head_tree
            .get_path(&link_path)
            .with_context(|| format!("Failed to find .josh-link.toml at path '{}'", path))?;

        let link_blob = repo
            .find_blob(link_entry.id())
            .context("Failed to find blob")?;

        let link_content = std::str::from_utf8(link_blob.content())
            .context("Failed to parse link file content")?;

        let link_file: JoshLinkFile =
            toml::from_str(link_content).context("Failed to parse .josh-link.toml")?;

        vec![(std::path::PathBuf::from(path), link_file)]
    } else {
        // No path specified - find all .josh-link.toml files in the tree
        josh_core::find_link_files(&repo, &head_tree)
            .map_err(from_josh_err)
            .context("Failed to find link files")?
    };

    if link_files.is_empty() {
        return Err(anyhow::anyhow!("No .josh-link.toml files found"));
    }

    println!("Found {} link file(s) to fetch", link_files.len());

    // Fetch from all the link files
    let mut updated_link_files = Vec::new();
    for (path, mut link_file) in link_files {
        println!("Fetching from link at path: {}", path.display());

        // Use git fetch shell command
        let output = std::process::Command::new("git")
            .args(&["fetch", &link_file.remote, &link_file.branch])
            .output()
            .context("Failed to execute git fetch")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
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
        let actual_commit_sha = fetch_commit.id();

        // Update the link file with the new commit SHA
        link_file.commit = Oid::from(actual_commit_sha);
        updated_link_files.push((path, link_file));
    }

    // Create new tree with updated .josh-link.toml files
    let mut new_tree = head_tree;
    for (path, link_file) in &updated_link_files {
        // Create the .josh-link.toml content
        let link_content = toml::to_string(link_file).context("Failed to serialize link file")?;

        // Create the blob for the .josh-link.toml file
        let link_blob = repo
            .blob(link_content.as_bytes())
            .context("Failed to create blob")?;

        // Create the path for the .josh-link.toml file
        let link_path = path.join(".josh-link.toml");

        // Insert the updated .josh-link.toml file into the tree
        new_tree = tree::insert(&repo, &new_tree, &link_path, link_blob, 0o0100644)
            .map_err(from_josh_err)
            .with_context(|| {
                format!(
                    "Failed to insert link file into tree at path '{}'",
                    path.display()
                )
            })?;
    }

    // Create a new commit with the updated tree
    let signature = if let Ok(time) = std::env::var("JOSH_COMMIT_TIME") {
        git2::Signature::new(
            "JOSH",
            "josh@josh-project.dev",
            &git2::Time::new(time.parse().context("Failed to parse JOSH_COMMIT_TIME")?, 0),
        )
        .context("Failed to create signature")?
    } else {
        repo.signature().context("Failed to get signature")?
    };

    let new_commit = repo
        .commit(
            None, // Don't update any reference
            &signature,
            &signature,
            &format!(
                "Update links: {}",
                updated_link_files
                    .iter()
                    .map(|(p, _)| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            &new_tree,
            &[&head_commit],
        )
        .context("Failed to create commit")?;

    // Apply the :link filter to the new commit
    let link_filter = josh_core::filter::parse(":link")
        .map_err(from_josh_err)
        .context("Failed to parse :link filter")?;

    // Load the cache and create transaction
    let repo_path = repo.path().parent().unwrap();
    josh_core::cache_sled::sled_load(&repo_path).unwrap();
    let cache = std::sync::Arc::new(
        josh_core::cache_stack::CacheStack::new()
            .with_backend(josh_core::cache_sled::SledCacheBackend::default())
            .with_backend(
                josh_core::cache_notes::NotesCacheBackend::new(&repo_path)
                    .map_err(from_josh_err)
                    .context("Failed to create NotesCacheBackend")?,
            ),
    );

    // Open Josh transaction
    let transaction = josh_core::cache::TransactionContext::from_env(cache.clone())
        .map_err(from_josh_err)
        .context("Failed TransactionContext::from_env")?
        .open(None)
        .map_err(from_josh_err)
        .context("Failed TransactionContext::open")?;
    let filtered_commit = josh_core::filter_commit(
        &transaction,
        link_filter,
        new_commit,
        josh_core::filter::empty(),
    )
    .map_err(from_josh_err)
    .context("Failed to apply :link filter")?;

    // Create the fixed branch name
    let branch_name = "refs/heads/josh-link";

    // Create or update the branch reference
    repo.reference(branch_name, filtered_commit, true, "josh link fetch")
        .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

    println!("Updated {} link file(s)", updated_link_files.len());
    println!("Created branch: {}", branch_name);

    Ok(())
}

fn handle_remote(args: &RemoteArgs) -> anyhow::Result<()> {
    match &args.command {
        RemoteCommand::Add(add_args) => handle_remote_add(add_args),
    }
}

fn handle_remote_add(args: &RemoteAddArgs) -> anyhow::Result<()> {
    // Check if we're in a git repository
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
    let repo_path = repo.path().parent().unwrap();

    handle_remote_add_repo(args, repo_path)
}

fn handle_remote_add_repo(args: &RemoteAddArgs, repo_path: &std::path::Path) -> anyhow::Result<()> {
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;

    // Store the remote information in josh-remote config instead of adding a git remote
    let remote_url = to_absolute_remote_url(&args.url)?;

    // Store the filter in git config per remote
    // Append ":prune=trivial-merge" to all filters unless --keep-trivial-merges flag is set
    let filter_to_store = if args.keep_trivial_merges {
        args.filter.clone()
    } else {
        format!("{}:prune=trivial-merge", args.filter)
    };

    let mut config = repo.config().context("Failed to get git config")?;

    // Store remote URL in josh-remote section
    config
        .set_str(&format!("josh-remote.{}.url", args.name), &remote_url)
        .context("Failed to store remote URL in git config")?;

    // Store filter in josh-remote section
    config
        .set_str(
            &format!("josh-remote.{}.filter", args.name),
            &filter_to_store,
        )
        .context("Failed to store filter in git config")?;

    // Store refspec in josh-remote section (for unfiltered refs)
    let refspec = format!("+refs/heads/*:refs/josh/remotes/{}/*", args.name);
    config
        .set_str(&format!("josh-remote.{}.fetch", args.name), &refspec)
        .context("Failed to store refspec in git config")?;

    // Set up a git remote that points to "." with a refspec to fetch filtered refs
    // Add remote pointing to current directory
    let repo_remote = to_absolute_remote_url(&repo_path.display().to_string())?;
    spawn_git_command(repo_path, &["remote", "add", &args.name, &repo_remote], &[])
        .context("Failed to add git remote")?;

    // Set up namespace configuration for the remote
    let namespace = format!("josh-{}", args.name);
    let uploadpack_cmd = format!("env GIT_NAMESPACE={} git upload-pack", namespace);

    spawn_git_command(
        repo_path,
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
fn handle_filter(args: &FilterArgs) -> anyhow::Result<()> {
    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
    let repo_path = repo.path().parent().unwrap().to_path_buf();

    handle_filter_repo(args, &repo_path, true)
}

/// Internal filter function that can be called from other handlers
fn handle_filter_repo(
    args: &FilterArgs,
    repo_path: &std::path::Path,
    print_messages: bool,
) -> anyhow::Result<()> {
    let repo = git2::Repository::open(repo_path).context("Failed to open repository")?;

    // Read the filter from git config for this remote
    let config = repo.config().context("Failed to get git config")?;

    let filter_key = format!("josh-remote.{}.filter", args.remote);
    let filter = config
        .get_string(&filter_key)
        .with_context(|| format!("No filter configured for remote '{}'", args.remote))?;

    if print_messages {
        println!("Applying filter '{}' to remote '{}'", filter, args.remote);
    }

    // Apply josh filtering (this is the same as in handle_fetch but without the git fetch step)
    apply_josh_filtering(repo_path, &filter, &args.remote)?;

    if print_messages {
        println!("Applied filter to remote: {}", args.remote);
    }

    Ok(())
}
