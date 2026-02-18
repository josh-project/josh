use anyhow::{Context, anyhow};

use josh_core::changes::{PushMode, build_to_push};
use josh_core::git::{normalize_repo_path, spawn_git_command};

use crate::config::{RemoteConfig, read_remote_config};

#[derive(Debug, clap::Parser)]
pub struct PushArgs {
    /// Remote name (or URL) to push to (optional, defaults to git's configured remote)
    ///
    /// When omitted, behaves like `git push` and uses the current branch's
    /// configured remote (or a reasonable default such as `origin`).
    #[arg()]
    pub remote: Option<String>,

    /// One or more refspecs to push (e.g. main, HEAD:refs/heads/main)
    ///
    /// These are positional arguments following the optional remote, matching
    /// `git push [<repository> [<refspec>...]]` syntax.
    #[arg()]
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

    /// Push mode (split or stack)
    #[command(flatten)]
    pub push_mode: PushModeArgs,
}

#[derive(Debug, clap::Args)]
#[group(multiple = false)]
pub struct PushModeArgs {
    /// Use split mode for pushing
    #[arg(long)]
    pub split: bool,

    /// Use stack mode for pushing
    #[arg(long)]
    pub stack: bool,
}

impl PushModeArgs {
    pub fn mode(&self) -> PushMode {
        if self.split {
            PushMode::Split
        } else if self.stack {
            PushMode::Stack
        } else {
            PushMode::Normal
        }
    }
}

pub fn handle_push(
    args: &PushArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    // Read remote configuration from .git/josh/remotes/<name>.josh
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    // Determine which remote to use:
    // - If a remote was explicitly provided, use it.
    // - Otherwise, fall back to a reasonable default (currently "origin"),
    //   similar to how `git push` uses the configured upstream when no
    //   repository argument is given.
    let remote_name = args.remote.as_deref().unwrap_or("origin");

    let RemoteConfig {
        url,
        filter_with_meta,
        ..
    } = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

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

        // Look up the josh remote reference once and derive both original_target
        // and old_filtered_oid from it
        let josh_remote_ref = format!("refs/josh/remotes/{}/{}", remote_name, remote_ref);
        let (original_target, old_filtered_oid) =
            if let Ok(remote_reference) = repo.find_reference(&josh_remote_ref) {
                let josh_remote_oid = remote_reference.target().unwrap_or(git2::Oid::zero());

                // Apply the filter to get the old filtered oid
                let (filtered_oids, errors) = josh_core::filter_refs(
                    transaction,
                    filter,
                    &[(josh_remote_ref.clone(), josh_remote_oid)],
                );

                if let Some(error) = errors.into_iter().next() {
                    return Err(anyhow!("josh filter error: {}", error.1));
                }

                let old_filtered = if let Some((_, filtered_oid)) = filtered_oids.first() {
                    *filtered_oid
                } else {
                    git2::Oid::zero()
                };

                (josh_remote_oid, old_filtered)
            } else {
                (git2::Oid::zero(), git2::Oid::zero())
            };

        log::debug!("old_filtered_oid: {:?}", old_filtered_oid);
        log::debug!("original_target: {:?}", original_target);

        let push_mode = args.push_mode.mode();

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

        log::debug!("unfiltered_oid: {:?}", unfiltered_oid);

        let to_push = build_to_push(
            transaction.repo(),
            changes,
            push_mode,
            &remote_ref,
            &author,
            &remote_ref,
            unfiltered_oid,
            original_target,
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
            let target_remote = url.clone();

            // Create refspec: oid:refname
            let push_refspec = format!("{}:{}", oid, refname);

            git_push_args.push(&target_remote);
            git_push_args.push(&push_refspec);

            // Use direct spawn so users can see git push progress
            if let Err(e) = spawn_git_command(
                repo.path(),
                &git_push_args, // Skip "git" since spawn_git_command adds it
                &[],
            ) {
                eprintln!("Failed to push {} to {}/{}", oid, remote_name, refname);
                eprintln!("{}", e);
            } else {
                eprintln!("Pushed {} to {}/{}", oid, remote_name, refname);
            }
        }
    }

    Ok(())
}
