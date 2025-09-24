#![warn(unused_extern_crates)]

use clap::Parser;
use josh::changes::{PushMode, build_to_push};
use josh::shell::Shell;
use std::io::IsTerminal;
use std::process::{Command as ProcessCommand, Stdio};

/// Spawn a git command directly to the terminal so users can see progress
/// Falls back to captured output if not in a TTY environment
fn spawn_git_command(
    cwd: &std::path::Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<i32, Box<dyn std::error::Error>> {
    let mut command = ProcessCommand::new("git");
    command.current_dir(cwd).args(args);

    // Add environment variables
    for (key, value) in env {
        command.env(key, value);
    }

    // Check if we're in a TTY environment
    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    if is_tty {
        // In TTY: inherit stdio so users can see progress
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = command.status()?;
        Ok(status.code().unwrap_or(1))
    } else {
        // Not in TTY: capture output (for tests, CI, etc.)
        // Use the same approach as josh::shell::Shell for consistency
        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("failed to execute git command: {}", e))?;

        Ok(output.status.code().unwrap_or(1))
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

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Command::Clone(args) => {
            if let Err(e) = handle_clone(args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Command::Fetch(args) => {
            if let Err(e) = handle_fetch(args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Command::Pull(args) => {
            if let Err(e) = handle_pull(args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Command::Push(args) => {
            if let Err(e) = handle_push(args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Command::Remote(args) => {
            if let Err(e) = handle_remote(args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Command::Filter(args) => {
            if let Err(e) = handle_filter(args) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

/// Apply josh filtering to all remote refs and update local refs
fn apply_josh_filtering(
    repo_shell: &Shell,
    filter: &str,
    remote_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Change to the repository directory
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(repo_shell.cwd.as_path())?;

    // Use josh API directly instead of calling josh-filter binary
    let filterobj =
        josh::filter::parse(filter).map_err(|e| format!("Failed to parse filter: {}", e.0))?;
    let transaction = josh::cache::Transaction::open_from_env(true)
        .map_err(|e| format!("Failed to open transaction: {}", e.0))?;
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
        return Err("No remote references found".into());
    }

    // Apply the filter to all remote refs
    let (updated_refs, errors) =
        josh::filter_refs(&transaction, filterobj, &input_refs, josh::filter::empty());

    // Check for errors
    for error in errors {
        return Err(format!("josh filter error: {}", error.1.0).into());
    }

    // Create filtered refs in refs/remotes/{remote_name}/* and refs/heads/*
    //let mut head_branch_name = None;

    //// First pass: find what HEAD points to
    //for (original_ref, _filtered_oid) in &updated_refs {
    //    println!("original_ref: {}", original_ref);
    //    if original_ref.ends_with("/HEAD") {
    //        // For HEAD, we need to resolve what branch it points to
    //        let head_ref = repo.find_reference(original_ref)?;
    //        if let Some(symbolic_target) = head_ref.symbolic_target() {
    //            // Extract the branch name from the symbolic target (e.g., "refs/heads/main" -> "main")
    //            if let Some(branch_name) = symbolic_target.strip_prefix("refs/heads/") {
    //                head_branch_name = Some(branch_name.to_string());
    //            }
    //        }
    //        break;
    //    }
    //}

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
            .ok_or("Invalid josh remote reference")?;

        // Create filtered remote reference
        let filtered_remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);
        repo.reference(&filtered_remote_ref, filtered_oid, true, "josh filter")
            .map_err(|e| format!("Failed to create filtered remote reference: {}", e))?;

        // Create filtered local branch (but skip HEAD)
        if branch_name != "HEAD" {
            let filtered_local_ref = format!("refs/heads/{}", branch_name);
            repo.reference(&filtered_local_ref, filtered_oid, true, "josh filter")
                .map_err(|e| format!("Failed to create filtered local reference: {}", e))?;
        }
    }

    //  // Set HEAD to point to the default branch and checkout the working directory
    //  if let Some(default_branch) = head_branch_name {
    //      let head_ref = format!("refs/heads/{}", default_branch);

    //      // Create a symbolic reference for HEAD pointing to the default branch
    //      repo.reference_symbolic("HEAD", &head_ref, true, "josh filter")
    //          .map_err(|e| format!("Failed to create symbolic HEAD reference: {}", e))?;

    //      // Checkout the working directory to the default branch
    //      let mut checkout_builder = git2::build::CheckoutBuilder::new();
    //      checkout_builder.force();
    //      repo.checkout_head(Some(&mut checkout_builder))
    //          .map_err(|e| format!("Failed to checkout HEAD: {}", e))?;
    //  }

    // Restore the original directory
    std::env::set_current_dir(original_dir)?;
    Ok(())
}

fn handle_clone(args: &CloneArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Use the provided output directory
    let output_dir = args.out.clone();

    // Create the output directory first
    std::fs::create_dir_all(&output_dir)?;

    // Initialize a new git repository inside the directory using git2
    let _repo = git2::Repository::init(&output_dir)
        .map_err(|e| format!("Failed to initialize git repository: {}", e))?;

    // Change to the repository directory and add the remote using handle_remote_add
    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&output_dir)?;

    // Make the URL absolute if it's a relative path (for local repositories)
    let absolute_url = if args.url.starts_with("http") || args.url.starts_with("ssh://") {
        args.url.clone()
    } else {
        // For local paths, make them absolute relative to the original directory
        let absolute_path = if args.url.starts_with('/') {
            // Already absolute
            args.url.clone()
        } else {
            // Relative to original directory
            original_dir.join(&args.url).to_string_lossy().to_string()
        };
        absolute_path
    };

    // Use handle_remote_add to add the remote with the filter
    let remote_add_args = RemoteAddArgs {
        name: "origin".to_string(),
        url: absolute_url,
        filter: args.filter.clone(),
        keep_trivial_merges: args.keep_trivial_merges,
    };
    handle_remote_add(&remote_add_args)?;

    // Create PullArgs from CloneArgs
    let pull_args = PullArgs {
        remote: "origin".to_string(),
        rref: args.branch.clone(),
        prune: false,
        ff_only: false,
    };

    // Use handle_pull to do the actual fetching and filtering
    let result = handle_pull(&pull_args);

    // Restore the original directory
    std::env::set_current_dir(original_dir)?;

    // Handle the result
    match result {
        Ok(_) => {
            println!("Cloned repository to: {}", output_dir.display());
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn handle_pull(args: &PullArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Check if we're in a git repository
    let repo =
        git2::Repository::open_from_env().map_err(|e| format!("Not in a git repository: {}", e))?;

    // Create FetchArgs from PullArgs
    let fetch_args = FetchArgs {
        remote: args.remote.clone(),
        rref: args.rref.clone(),
        prune: args.prune,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch(&fetch_args)?;

    let head_ref = repo.find_reference(&format!("refs/remotes/{}/HEAD", args.remote))?;
    let head_target = head_ref
        .symbolic_target()
        .unwrap()
        .replace("refs/remotes/origin", "refs/heads");

    repo.set_head(&head_target)
        .map_err(|e| format!("Failed to set HEAD: {}", e))?;

    // After fetching, we need to checkout the updated content
    // Get the current branch name
    let repo = git2::Repository::open_from_env()
        .map_err(|e| format!("Failed to open git repository: {}", e))?;

    let head = repo
        .head()
        .map_err(|e| format!("Failed to get HEAD: {}", e))?;

    let current_branch = head
        .shorthand()
        .ok_or("Failed to get current branch name")?;

    // Checkout the updated filtered content
    let branch_ref = format!("refs/heads/{}", current_branch);
    repo.set_head(&branch_ref)
        .map_err(|e| format!("Failed to set HEAD: {}", e))?;

    // Update the working directory to match the HEAD
    let mut checkout_builder = git2::build::CheckoutBuilder::new();
    checkout_builder.force();
    repo.checkout_head(Some(&mut checkout_builder))
        .map_err(|e| format!("Failed to checkout updated content: {}", e))?;

    println!("Pulled from remote: {}", args.remote);
    Ok(())
}

fn handle_fetch(args: &FetchArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Get current working directory (should be inside a git repository)
    let current_dir = std::env::current_dir()?;

    // Create shell for the current repository directory
    let repo_shell = Shell {
        cwd: current_dir.clone(),
    };

    // Get PATH environment variable for shell commands
    let path_env = std::env::var("PATH").unwrap_or_default();

    // First, fetch unfiltered refs to refs/josh/remotes/*
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &["fetch", &args.remote],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!(
            "git fetch to josh/remotes failed with exit code: {}",
            exit_code
        )
        .into());
    }

    // Apply josh filtering using handle_filter_internal (without messages)
    let filter_args = FilterArgs {
        remote: args.remote.clone(),
    };
    handle_filter_internal(&filter_args, false)?;
    // Note: fetch doesn't checkout, it just updates the refs

    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &["remote", "set-head", &args.remote, "--auto"],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!("git remote set-head failed with exit code: {}", exit_code).into());
    }

    println!("Fetched from remote: {}", args.remote);
    Ok(())
}

fn handle_push(args: &PushArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Get current working directory (should be inside a git repository)
    let current_dir = std::env::current_dir()?;

    // Create shell for the current repository directory
    let repo_shell = Shell {
        cwd: current_dir.clone(),
    };

    // Get PATH environment variable for shell commands
    let path_env = std::env::var("PATH").unwrap_or_default();

    // Check if we're in a git repository
    let repo =
        git2::Repository::open_from_env().map_err(|e| format!("Not in a git repository: {}", e))?;

    // Read filter from git config for the specific remote
    let config = repo
        .config()
        .map_err(|e| format!("Failed to get git config: {}", e))?;

    let filter_str = config
        .get_string(&format!("remote.{}.josh-filter", args.remote))
        .map_err(|e| format!("Failed to read filter from git config: {}", e))?;

    // Parse the filter using Josh API
    let filter =
        josh::filter::parse(&filter_str).map_err(|e| format!("Failed to parse filter: {}", e.0))?;

    // Open Josh transaction
    let transaction = josh::cache::Transaction::open_from_env(true)
        .map_err(|e| format!("Failed to open transaction: {}", e.0))?;

    // If no refspecs provided, push the current branch
    let refspecs = if args.refspecs.is_empty() {
        // Get the current branch name
        let head = repo
            .head()
            .map_err(|e| format!("Failed to get HEAD: {}", e))?;

        let current_branch = head
            .shorthand()
            .ok_or("Failed to get current branch name")?;

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
            .map_err(|e| format!("Failed to resolve local ref '{}': {}", local_ref, e))?
            .target()
            .ok_or("Failed to get target of local ref")?;

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

        // For the old filtered oid, we need to find the previous filtered commit
        // This should be the parent of the current commit, or the remote tracking branch
        let remote_tracking_ref = format!("refs/remotes/{}/{}", args.remote, remote_ref);
        let old_filtered_oid =
            if let Ok(remote_reference) = repo.find_reference(&remote_tracking_ref) {
                // Use the remote tracking branch as the old filtered commit
                remote_reference.target().unwrap_or(git2::Oid::zero())
            } else {
                // If no remote tracking branch, use the parent of the current commit
                if let Ok(commit) = repo.find_commit(local_commit) {
                    if let Ok(parent) = commit.parent(0) {
                        parent.id()
                    } else {
                        git2::Oid::zero()
                    }
                } else {
                    git2::Oid::zero()
                }
            };

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

        let mut changes: Option<Vec<josh::Change>> =
            if push_mode == PushMode::Stack || push_mode == PushMode::Split {
                Some(vec![])
            } else {
                None
            };

        // Use Josh API to unapply the filter
        let unfiltered_oid = josh::history::unapply_filter(
            &transaction,
            filter,
            original_target,
            old_filtered_oid,
            local_commit,
            false,        // keep_orphans
            None,         // reparent_orphans
            &mut changes, // change_ids
        )
        .map_err(|e| format!("Failed to unapply filter: {}", e.0))?;

        // Define variables needed for build_to_push
        let baseref = remote_ref.clone();
        let ref_with_options = if args.force || args.atomic {
            format!(
                "{}{}{}",
                remote_ref,
                "%",
                if args.force { "force" } else { "" }
            )
        } else {
            remote_ref.clone()
        };
        let oid_to_push = unfiltered_oid;
        let old = original_target;

        let to_push = build_to_push(
            transaction.repo(),
            changes,
            push_mode,
            &baseref,
            &author,
            ref_with_options,
            oid_to_push,
            old,
        )
        .map_err(|e| format!("Failed to build to push: {}", e.0))?;

        // Process each entry in to_push (similar to josh-proxy)
        for (refname, oid, repo) in to_push {
            // Build git push command
            let mut git_push_cmd = vec!["git", "push"];

            if args.force {
                git_push_cmd.push("--force");
            }

            if args.atomic {
                git_push_cmd.push("--atomic");
            }

            if args.dry_run {
                git_push_cmd.push("--dry-run");
            }

            // Determine the target remote
            let target_remote = if !repo.is_empty()
                && (args.remote.starts_with("http")
                    || args.remote.starts_with("ssh://")
                    || args.remote.starts_with("/"))
            {
                // If repo is specified and remote is a URL, construct URL with repo path
                format!("{}/{}", args.remote, repo)
            } else {
                // If remote is a name or repo is empty, use the remote name directly
                args.remote.clone()
            };

            git_push_cmd.push(&target_remote);

            // Create refspec: oid:refname
            let push_refspec = format!("{}:{}", oid, refname);
            git_push_cmd.push(&push_refspec);

            // Use direct spawn so users can see git push progress
            let exit_code = spawn_git_command(
                repo_shell.cwd.as_path(),
                &git_push_cmd[1..], // Skip "git" since spawn_git_command adds it
                &[("PATH", &path_env)],
            )?;

            if exit_code != 0 {
                return Err(
                    format!("git push failed for {}: exit code {}", refname, exit_code).into(),
                );
            }

            println!("Pushed {} to {}/{}", oid, target_remote, refname);
        }
    }

    Ok(())
}

fn handle_remote(args: &RemoteArgs) -> Result<(), Box<dyn std::error::Error>> {
    match &args.command {
        RemoteCommand::Add(add_args) => handle_remote_add(add_args),
    }
}

fn handle_remote_add(args: &RemoteAddArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Check if we're in a git repository
    let repo =
        git2::Repository::open_from_env().map_err(|e| format!("Not in a git repository: {}", e))?;

    // Add the remote using git2 API
    let remote_path = if args.url.starts_with("http") || args.url.starts_with("ssh://") {
        args.url.clone()
    } else {
        // For local paths, make them absolute
        std::fs::canonicalize(&args.url)
            .map_err(|e| format!("Failed to resolve path {}: {}", args.url, e))?
            .to_string_lossy()
            .to_string()
    };

    repo.remote(&args.name, &remote_path)
        .map_err(|e| format!("Failed to add remote '{}': {}", args.name, e))?;

    // Remove default refspecs and add only josh-specific refspecs
    let repo_shell = Shell {
        cwd: repo.path().parent().unwrap().to_path_buf(),
    };

    // Remove the default refspec that git automatically adds
    let (_stdout, _stderr, _exit_code) = repo_shell.command_env(
        &[
            "git",
            "config",
            "--unset-all",
            &format!("remote.{}.fetch", args.name),
        ],
        &[("PATH", &std::env::var("PATH").unwrap_or_default())],
        &[],
    );

    // It's okay if this fails (no refspecs to remove)
    // We continue regardless of the exit code

    // Add josh-specific refspecs
    // Fetch all branches to refs/josh/remotes/{remote_name}/*
    let (stdout, stderr, exit_code) = repo_shell.command_env(
        &[
            "git",
            "config",
            "--add",
            &format!("remote.{}.fetch", args.name),
            &format!("refs/heads/*:refs/josh/remotes/{}/*", args.name),
        ],
        &[("PATH", &std::env::var("PATH").unwrap_or_default())],
        &[],
    );

    if exit_code != 0 {
        return Err(format!("Failed to add fetch refspec: {}\n{}", stdout, stderr).into());
    }

    if exit_code != 0 {
        return Err(format!("Failed to add HEAD fetch refspec: {}\n{}", stdout, stderr).into());
    }

    // Store the filter in git config per remote
    // Append ":prune=trivial-merge" to all filters unless --keep-trivial-merges flag is set
    let filter_to_store = if args.keep_trivial_merges {
        args.filter.clone()
    } else {
        format!("{}:prune=trivial-merge", args.filter)
    };

    let mut config = repo
        .config()
        .map_err(|e| format!("Failed to get git config: {}", e))?;

    config
        .set_str(
            &format!("remote.{}.josh-filter", args.name),
            &filter_to_store,
        )
        .map_err(|e| format!("Failed to store filter in git config: {}", e))?;

    println!(
        "Added remote '{}' with filter '{}'",
        args.name, filter_to_store
    );

    Ok(())
}

/// Handle the `josh filter` command - apply filtering to existing refs without fetching
fn handle_filter(args: &FilterArgs) -> Result<(), Box<dyn std::error::Error>> {
    handle_filter_internal(args, true)
}

/// Internal filter function that can be called from other handlers
fn handle_filter_internal(
    args: &FilterArgs,
    print_messages: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = git2::Repository::open_from_env()?;
    let repo_shell = Shell {
        cwd: repo.path().parent().unwrap().to_path_buf(),
    };

    // Read the filter from git config for this remote
    let config = repo
        .config()
        .map_err(|e| format!("Failed to get git config: {}", e))?;

    let filter_key = format!("remote.{}.josh-filter", args.remote);
    let filter = config
        .get_string(&filter_key)
        .map_err(|e| format!("No filter configured for remote '{}': {}", args.remote, e))?;

    if print_messages {
        println!("Applying filter '{}' to remote '{}'", filter, args.remote);
    }

    // Apply josh filtering (this is the same as in handle_fetch but without the git fetch step)
    apply_josh_filtering(&repo_shell, &filter, &args.remote)?;

    if print_messages {
        println!("Applied filter to remote: {}", args.remote);
    }

    Ok(())
}
