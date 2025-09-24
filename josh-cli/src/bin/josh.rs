#![warn(unused_extern_crates)]

use clap::Parser;
use josh::changes::{PushMode, build_to_push};
use josh::shell::Shell;
use log::debug;
use std::io::IsTerminal;
use std::process::{Command as ProcessCommand, Stdio};

/// Spawn a git command directly to the terminal so users can see progress
/// Falls back to captured output if not in a TTY environment
fn spawn_git_command(
    cwd: &std::path::Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<i32, Box<dyn std::error::Error>> {
    debug!("spawn_git_command: {:?}", args);
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
        // Not in TTY: capture output and print stderr (for tests, CI, etc.)
        // Use the same approach as josh::shell::Shell for consistency
        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("failed to execute git command: {}", e))?;

        // Print stderr if there's any output
        if !output.stderr.is_empty() {
            let output_str = String::from_utf8_lossy(&output.stderr);
            let output_str = if let Ok(testtmp) = std::env::var("TESTTMP") {
                //println!("TESTTMP {:?}", testtmp);
                output_str.replace(&testtmp, "$TESTTMP")
            } else {
                output_str.to_string()
            };
            eprintln!("{}", output_str);
        }

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

fn main() {
    env_logger::init();
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

        // Create filtered reference in josh/filtered namespace
        let filtered_ref = format!(
            "refs/namespaces/josh-{}/refs/heads/{}",
            remote_name, branch_name
        );
        repo.reference(&filtered_ref, filtered_oid, true, "josh filter")
            .map_err(|e| format!("Failed to create filtered reference: {}", e))?;
    }

    // Fetch the filtered refs to create standard remote refs
    let path_env = std::env::var("PATH").unwrap_or_default();
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &["fetch", remote_name],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!("Failed to fetch filtered refs: exit code {}", exit_code).into());
    }

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

    // Create FetchArgs from CloneArgs
    let fetch_args = FetchArgs {
        remote: "origin".to_string(),
        rref: args.branch.clone(),
        prune: false,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch(&fetch_args)?;

    // Get the default branch name from the remote HEAD symref
    let default_branch = if args.branch == "HEAD" {
        // Read the remote HEAD symref to get the default branch
        let head_ref = format!("refs/remotes/origin/HEAD");
        let repo = git2::Repository::open_from_env()
            .map_err(|e| format!("Not in a git repository: {}", e))?;

        let head_reference = repo
            .find_reference(&head_ref)
            .map_err(|e| format!("Failed to find remote HEAD reference {}: {}", head_ref, e))?;

        let symref_target = head_reference
            .symbolic_target()
            .ok_or("Remote HEAD reference is not a symbolic reference")?;

        // Extract branch name from symref target (e.g., "refs/remotes/origin/master" -> "master")
        let branch_name = symref_target
            .strip_prefix("refs/remotes/origin/")
            .ok_or_else(|| format!("Invalid symref target format: {}", symref_target))?;

        branch_name.to_string()
    } else {
        args.branch.clone()
    };

    // Checkout the default branch
    let path_env = std::env::var("PATH").unwrap_or_default();
    let repo_shell = Shell {
        cwd: std::env::current_dir()?,
    };

    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &[
            "checkout",
            "-b",
            &default_branch,
            &format!("origin/{}", default_branch),
        ],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!(
            "Failed to checkout branch {}: exit code {}",
            default_branch, exit_code
        )
        .into());
    }

    // Set up upstream tracking for the branch
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &[
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", default_branch),
            &default_branch,
        ],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!(
            "Failed to set upstream for branch {}: exit code {}",
            default_branch, exit_code
        )
        .into());
    }

    // Restore the original directory
    std::env::set_current_dir(original_dir)?;

    println!("Cloned repository to: {}", output_dir.display());
    Ok(())
}

fn handle_pull(args: &PullArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Check if we're in a git repository
    let _repo =
        git2::Repository::open_from_env().map_err(|e| format!("Not in a git repository: {}", e))?;

    // Create FetchArgs from PullArgs
    let fetch_args = FetchArgs {
        remote: args.remote.clone(),
        rref: args.rref.clone(),
        prune: args.prune,
    };

    // Use handle_fetch to do the actual fetching and filtering
    handle_fetch(&fetch_args)?;

    // Get current working directory for shell commands
    let current_dir = std::env::current_dir()?;
    let repo_shell = Shell {
        cwd: current_dir.clone(),
    };
    let path_env = std::env::var("PATH").unwrap_or_default();

    // Get the default branch name from the remote HEAD symref
    //let default_branch = if args.rref == "HEAD" {
    //    // Read the remote HEAD symref to get the default branch
    //    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);
    //    let repo = git2::Repository::open_from_env()
    //        .map_err(|e| format!("Not in a git repository: {}", e))?;

    //    let head_reference = repo
    //        .find_reference(&head_ref)
    //        .map_err(|e| format!("Failed to find remote HEAD reference {}: {}", head_ref, e))?;

    //    let symref_target = head_reference
    //        .symbolic_target()
    //        .ok_or("Remote HEAD reference is not a symbolic reference")?;

    //    // Extract branch name from symref target (e.g., "refs/remotes/origin/master" -> "master")
    //    let branch_name = symref_target
    //        .strip_prefix(&format!("refs/remotes/{}/", args.remote))
    //        .ok_or_else(|| format!("Invalid symref target format: {}", symref_target))?;

    //    branch_name.to_string()
    //} else {
    //    args.rref.clone()
    //};

    // Now use actual git pull to integrate the changes
    let mut git_cmd = vec!["git", "pull"];

    // Add flags based on arguments
    if args.rebase {
        git_cmd.push("--rebase");
    }

    if args.autostash {
        git_cmd.push("--autostash");
    }

    // Add the remote and branch in the format: git pull {remote} {remote}/{branch}
    //let remote_branch = format!("{}/{}", args.remote, default_branch);
    git_cmd.push(&args.remote);
    //git_cmd.push(&remote_branch);

    // Execute the git pull command
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &git_cmd[1..], // Skip "git" since spawn_git_command adds it
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!("git pull failed with exit code: {}", exit_code).into());
    }

    println!("Pulled from remote: {}", args.remote);
    Ok(())
}

fn handle_fetch(args: &FetchArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Check if we're in a git repository
    let repo =
        git2::Repository::open_from_env().map_err(|e| format!("Not in a git repository: {}", e))?;

    // Get current working directory (should be inside a git repository)
    let current_dir = std::env::current_dir()?;

    // Create shell for the current repository directory
    let repo_shell = Shell {
        cwd: current_dir.clone(),
    };

    // Get PATH environment variable for shell commands
    let path_env = std::env::var("PATH").unwrap_or_default();

    // Read the remote URL from josh-remote config
    let config = repo
        .config()
        .map_err(|e| format!("Failed to get git config: {}", e))?;

    let remote_url = config
        .get_string(&format!("josh-remote.{}.url", args.remote))
        .map_err(|e| format!("Failed to get remote URL for '{}': {}", args.remote, e))?;

    let refspec = config
        .get_string(&format!("josh-remote.{}.fetch", args.remote))
        .map_err(|e| format!("Failed to get refspec for '{}': {}", args.remote, e))?;

    // First, fetch unfiltered refs to refs/josh/remotes/*
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &["fetch", &remote_url, &refspec],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!(
            "git fetch to josh/remotes failed with exit code: {}",
            exit_code
        )
        .into());
    }

    // Set up remote HEAD reference using git ls-remote
    // This is the proper way to get the default branch from the remote
    let head_ref = format!("refs/remotes/{}/HEAD", args.remote);

    // Use git ls-remote --symref to get the default branch
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &["ls-remote", "--symref", &remote_url, "HEAD"],
        &[("PATH", &path_env)],
    )?;

    if exit_code == 0 {
        // Parse the output to get the default branch name
        // Output format: "ref: refs/heads/main\t<commit-hash>"
        let output = std::process::Command::new("git")
            .args(&["ls-remote", "--symref", &remote_url, "HEAD"])
            .current_dir(repo_shell.cwd.as_path())
            .output()?;

        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = output_str.lines().next() {
                if let Some(symref_part) = line.split('\t').next() {
                    if symref_part.starts_with("ref: refs/heads/") {
                        let default_branch = &symref_part[16..]; // Remove "ref: refs/heads/"
                        let default_branch_ref =
                            format!("refs/remotes/{}/{}", args.remote, default_branch);

                        // Create the symbolic reference
                        let _ = repo.reference_symbolic(
                            &head_ref,
                            &default_branch_ref,
                            true,
                            "josh remote HEAD",
                        );
                        let _ = repo.reference_symbolic(
                            &format!("refs/namespaces/josh-{}/{}", args.remote, "HEAD"),
                            &format!("refs/heads/{}", default_branch),
                            true,
                            "josh remote HEAD",
                        );
                    }
                }
            }
        }
    }

    // Apply josh filtering using handle_filter_internal (without messages)
    let filter_args = FilterArgs {
        remote: args.remote.clone(),
    };
    handle_filter_internal(&filter_args, false)?;
    // Note: fetch doesn't checkout, it just updates the refs

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

    // Step 1: Push to the local repo (this will push to the filtered refs)
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

    // Push to the local remote (which points to ".")
    git_push_cmd.push(&args.remote);

    // Add refspecs if provided
    if !args.refspecs.is_empty() {
        for refspec in &args.refspecs {
            git_push_cmd.push(refspec);
        }
    }

    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &git_push_cmd[1..], // Skip "git" since spawn_git_command adds it
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!(
            "git push to local remote failed with exit code: {}",
            exit_code
        )
        .into());
    }

    // Step 2: Apply reverse filtering and push to actual remote
    let filter_str = config
        .get_string(&format!("josh-remote.{}.filter", args.remote))
        .map_err(|e| format!("Failed to read filter from git config: {}", e))?;

    // Parse the filter using Josh API
    let filter =
        josh::filter::parse(&filter_str).map_err(|e| format!("Failed to parse filter: {}", e.0))?;

    // Open Josh transaction
    let transaction = josh::cache::Transaction::open_from_env(true)
        .map_err(|e| format!("Failed to open transaction: {}", e.0))?;

    // Get the remote URL from josh-remote config
    let remote_url = config
        .get_string(&format!("josh-remote.{}.url", args.remote))
        .map_err(|e| format!("Failed to get remote URL for '{}': {}", args.remote, e))?;

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

        // Get the old filtered oid by applying the filter to the original remote ref
        // before we push to the namespace
        let josh_remote_ref = format!("refs/josh/remotes/{}/{}", args.remote, remote_ref);
        let old_filtered_oid =
            if let Ok(josh_remote_reference) = repo.find_reference(&josh_remote_ref) {
                let josh_remote_oid = josh_remote_reference.target().unwrap_or(git2::Oid::zero());

                // Apply the filter to the josh remote ref to get the old filtered oid
                let (filtered_oids, errors) = josh::filter_refs(
                    &transaction,
                    filter,
                    &[(josh_remote_ref.clone(), josh_remote_oid)],
                    josh::filter::empty(),
                );

                // Check for errors
                for error in errors {
                    return Err(format!("josh filter error: {}", error.1.0).into());
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

        debug!("old_filtered_oid: {:?}", old_filtered_oid);
        debug!("original_target: {:?}", original_target);

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

        debug!("unfiltered_oid: {:?}", unfiltered_oid);

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

        debug!("to_push: {:?}", to_push);

        // Process each entry in to_push (similar to josh-proxy)
        for (refname, oid, _) in to_push {
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

            // Determine the target remote URL
            let target_remote = remote_url.clone();

            // Create refspec: oid:refname
            let push_refspec = format!("{}:{}", oid, refname);

            git_push_cmd.push(&target_remote);
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

            println!("Pushed {} to {}/{}", oid, args.remote, refname);
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

    // Store the remote information in josh-remote config instead of adding a git remote
    let remote_path = if args.url.starts_with("http") || args.url.starts_with("ssh://") {
        args.url.clone()
    } else {
        // For local paths, make them absolute
        std::fs::canonicalize(&args.url)
            .map_err(|e| format!("Failed to resolve path {}: {}", args.url, e))?
            .to_string_lossy()
            .to_string()
    };

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

    // Store remote URL in josh-remote section
    config
        .set_str(&format!("josh-remote.{}.url", args.name), &remote_path)
        .map_err(|e| format!("Failed to store remote URL in git config: {}", e))?;

    // Store filter in josh-remote section
    config
        .set_str(
            &format!("josh-remote.{}.filter", args.name),
            &filter_to_store,
        )
        .map_err(|e| format!("Failed to store filter in git config: {}", e))?;

    // Store refspec in josh-remote section (for unfiltered refs)
    let refspec = format!("refs/heads/*:refs/josh/remotes/{}/*", args.name);
    config
        .set_str(&format!("josh-remote.{}.fetch", args.name), &refspec)
        .map_err(|e| format!("Failed to store refspec in git config: {}", e))?;

    // Set up a git remote that points to "." with a refspec to fetch filtered refs
    let path_env = std::env::var("PATH").unwrap_or_default();
    let current_dir = std::env::current_dir()?;
    let repo_shell = Shell {
        cwd: current_dir.clone(),
    };

    // Set receive.denyCurrentBranch=ignore to allow pushing to current branch
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &["config", "receive.denyCurrentBranch", "ignore"],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!(
            "Failed to set receive.denyCurrentBranch: exit code {}",
            exit_code
        )
        .into());
    }

    // Add remote pointing to current directory
    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &[
            "remote",
            "add",
            &args.name,
            &format!("file://{}", repo_shell.cwd.to_string_lossy()),
        ],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!("Failed to add git remote: exit code {}", exit_code).into());
    }

    // Set up namespace configuration for the remote
    let namespace = format!("josh-{}", args.name);
    let uploadpack_cmd = format!("env GIT_NAMESPACE={} git upload-pack", namespace);
    let receivepack_cmd = format!("env GIT_NAMESPACE={} git receive-pack", namespace);

    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &[
            "config",
            &format!("remote.{}.uploadpack", args.name),
            &uploadpack_cmd,
        ],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!("Failed to set remote uploadpack: exit code {}", exit_code).into());
    }

    let exit_code = spawn_git_command(
        repo_shell.cwd.as_path(),
        &[
            "config",
            &format!("remote.{}.receivepack", args.name),
            &receivepack_cmd,
        ],
        &[("PATH", &path_env)],
    )?;

    if exit_code != 0 {
        return Err(format!("Failed to set remote receivepack: exit code {}", exit_code).into());
    }

    // Set up refspec to fetch filtered refs to standard remote refs
    //let refspec = format!("refs/heads/*:refs/remotes/{}/*", args.name);
    //let exit_code = spawn_git_command(
    //    repo_shell.cwd.as_path(),
    //    &[
    //        "config",
    //        "--add",
    //        &format!("remote.{}.fetch", args.name),
    //        &refspec,
    //    ],
    //    &[("PATH", &path_env)],
    //)?;

    //if exit_code != 0 {
    //    return Err(format!("Failed to set remote refspec: exit code {}", exit_code).into());
    //}

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

    let filter_key = format!("josh-remote.{}.filter", args.remote);
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
