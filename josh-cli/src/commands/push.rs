use std::collections::HashMap;

use anyhow::{Context, anyhow};

use josh_core::changes::{PushMode, build_to_push};
use josh_core::git::{normalize_repo_path, spawn_git_command};

use crate::commands::auth::get_github_access_token;
use crate::config::{RemoteConfig, read_remote_config};
use crate::forge::{Forge, parse_github_owner_repo};

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
        forge,
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
        for (refname, oid, _) in &to_push {
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

        // If push mode is split or stacked and forge is GitHub, create or update PRs using @base as base branch
        if !args.dry_run
            && (push_mode == PushMode::Split || push_mode == PushMode::Stack)
            && forge == Some(Forge::Github)
        {
            let pr_infos = collect_pr_infos(repo, &to_push);
            if !pr_infos.is_empty() {
                if let Err(e) = create_or_update_github_prs(&url, &pr_infos) {
                    eprintln!("Warning: failed to create/update GitHub PRs: {}", e);
                }
            }
        }
    }

    Ok(())
}

/// One PR to create or update: head branch, base branch, title (from commit), body (from commit).
struct PrInfo {
    pub head_branch: String,
    pub base_branch: String,
    pub title: String,
    pub body: String,
}

/// Collect (head_branch, base_branch, title, body) from to_push for PR create/update.
/// Uses the @base ref for each change as the base branch. Title and body come from the head commit message.
fn collect_pr_infos(
    repo: &git2::Repository,
    to_push: &[(String, git2::Oid, String)],
) -> Vec<PrInfo> {
    fn branch_name(refname: &str) -> &str {
        refname.strip_prefix("refs/heads/").unwrap_or(refname)
    }
    let mut by_id: HashMap<String, (Option<String>, Option<String>, Option<git2::Oid>)> =
        HashMap::new();
    for (refname, oid, id) in to_push {
        let branch = branch_name(refname).to_string();
        if refname.contains("@changes") {
            let entry = by_id.entry(id.clone()).or_default();
            entry.0 = Some(branch);
            entry.2 = Some(*oid);
        } else if refname.contains("@base") {
            by_id.entry(id.clone()).or_default().1 = Some(branch);
        }
    }
    by_id
        .into_iter()
        .filter_map(|(_, (head, base, head_oid))| {
            let (head, base, head_oid) = (head?, base?, head_oid?);
            let commit = repo.find_commit(head_oid).ok()?;
            let raw_message = commit.message().unwrap_or("");
            let message = raw_message.trim_end();
            let title = message.lines().next().unwrap_or("").trim().to_string();
            let title = if title.is_empty() {
                format!("{} → {}", head, base)
            } else {
                title
            };
            let body = message.to_string();
            Some(PrInfo {
                head_branch: head,
                base_branch: base,
                title,
                body,
            })
        })
        .collect()
}

fn create_or_update_github_prs(url: &str, pr_infos: &[PrInfo]) -> anyhow::Result<()> {
    let token = match get_github_access_token()? {
        Some(t) => t,
        None => {
            eprintln!("No GitHub token found. Run 'josh auth login github' to create PRs.");
            return Ok(());
        }
    };
    let (owner, repo_name) = parse_github_owner_repo(url)?;
    let connection = josh_github_graphql::connection::GithubApiConnection::with_token(token)?;

    let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
    rt.block_on(async {
        let repository_id = connection.get_repo_id(&owner, &repo_name).await?;
        for info in pr_infos {
            match connection
                .find_pull_request_by_head(&owner, &repo_name, &info.head_branch)
                .await
            {
                Ok(Some((pr_id, number))) => {
                    match connection
                        .update_pull_request(
                            &pr_id,
                            Some(&info.title),
                            Some(&info.body),
                            Some(&info.base_branch),
                        )
                        .await
                    {
                        Ok((_, _)) => eprintln!(
                            "Updated PR #{}: {} (base: {})",
                            number, info.head_branch, info.base_branch
                        ),
                        Err(e) => {
                            let msg = e.to_string();
                            eprintln!("Failed to update PR #{} {}: {}", number, info.head_branch, msg);
                            if msg.contains("Resource not accessible by integration") {
                                eprintln!("Hint: set GITHUB_TOKEN to a Personal Access Token (repo + pull request) and try again.");
                            }
                        }
                    }
                }
                Ok(None) => {
                    match connection
                        .create_pull_request(
                            &repository_id,
                            &info.base_branch,
                            &info.head_branch,
                            &info.title,
                            &info.body,
                        )
                        .await
                    {
                        Ok((_, number)) => eprintln!(
                            "Created PR #{}: {} → {}",
                            number, info.head_branch, info.base_branch
                        ),
                        Err(e) => {
                            let msg = e.to_string();
                            eprintln!(
                                "Failed to create PR {} → {}: {}",
                                info.head_branch, info.base_branch, msg
                            );
                            if msg.contains("Resource not accessible by integration") {
                                eprintln!("Hint: set GITHUB_TOKEN to a Personal Access Token (repo + pull request) and try again.");
                            }
                        }
                    }
                }
                Err(e) => eprintln!("Failed to look up PR for {}: {}", info.head_branch, e),
            }
        }
        Ok::<(), anyhow::Error>(())
    })
}
