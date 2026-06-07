use anyhow::{Context, anyhow};

use josh_changes::{PushMode, PushRef, build_to_push};
use josh_core::git::{normalize_repo_path, spawn_git_command};

use crate::config::{RemoteConfig, read_remote_config};
use crate::forge::Forge;

#[derive(Debug, clap::Parser)]
pub struct PushArgs {
    /// Josh remote name to push to (optional, defaults to "origin")
    ///
    /// Must match a remote configured in `.git/config` with a josh filter
    /// (e.g. `josh-remote = origin`). Does not support bare URLs.
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

    /// Remote branch to use as the base for reverse filtering
    ///
    /// By default the destination branch is used. Pass --base to base
    /// the push on a different branch — typically when pushing a new
    /// branch that does not yet exist on the remote.
    #[arg(long = "base")]
    pub base: Option<String>,
}

#[derive(Debug, clap::Parser)]
pub struct PublishArgs {
    /// Josh remote name to push to (optional, defaults to "origin")
    ///
    /// Must match a remote configured in `.git/config` with a josh filter
    /// (e.g. `josh-remote = origin`). Does not support bare URLs.
    #[arg()]
    pub remote: Option<String>,

    /// One or more refspecs to push (e.g. main, HEAD:refs/heads/main)
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

    /// Remote branch to use as the base for reverse filtering
    ///
    /// See `josh push --base` for details.
    #[arg(long = "base")]
    pub base: Option<String>,
}

struct PreparedPush {
    to_push: Vec<PushRef>,
    pr_infos: Vec<josh_github_changes::PrInfo>,
}

fn prepare_push(
    refspec: &str,
    remote_name: &str,
    base: Option<&str>,
    transaction: &josh_core::cache::Transaction,
    filter: josh_core::filter::Filter,
    push_mode: &PushMode,
    forge: &Option<Forge>,
) -> anyhow::Result<PreparedPush> {
    let repo = transaction.repo();

    let (local_ref, remote_ref) = if let Some(colon_pos) = refspec.find(':') {
        let local = &refspec[..colon_pos];
        let remote = &refspec[colon_pos + 1..];
        (local.to_string(), remote.to_string())
    } else {
        (refspec.to_string(), refspec.to_string())
    };

    let remote_ref = remote_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&remote_ref);

    let local_commit = repo
        .resolve_reference_from_short_name(&local_ref)
        .with_context(|| format!("Failed to resolve local ref '{}'", local_ref))?
        .target()
        .context("Failed to get target of local ref")?;

    let dest_remote_ref = format!("refs/josh/remotes/{}/{}", remote_name, remote_ref);
    let (dest_oid, old_filtered_oid) =
        if let Ok(remote_reference) = repo.find_reference(&dest_remote_ref) {
            let dest_oid = remote_reference.target().unwrap_or(git2::Oid::zero());

            let (filtered_oids, errors) =
                josh_core::filter_refs(transaction, filter, &[(dest_remote_ref.clone(), dest_oid)]);

            if let Some(error) = errors.into_iter().next() {
                return Err(anyhow!("josh filter error: {}", error.1));
            }

            let old_filtered = if let Some((_, filtered_oid)) = filtered_oids.first() {
                *filtered_oid
            } else {
                git2::Oid::zero()
            };

            (dest_oid, old_filtered)
        } else {
            (git2::Oid::zero(), git2::Oid::zero())
        };

    let original_target = if let Some(base) = base {
        let base_remote_ref = format!("refs/josh/remotes/{}/{}", remote_name, base);
        repo.find_reference(&base_remote_ref)
            .with_context(|| {
                format!(
                    "Failed to resolve --base ref (looked up '{}')",
                    base_remote_ref
                )
            })?
            .target()
            .with_context(|| format!("Base ref '{}' has no target", base_remote_ref))?
    } else {
        dest_oid
    };

    log::debug!("old_filtered_oid: {:?}", old_filtered_oid);
    log::debug!("original_target: {:?}", original_target);

    let unfiltered_oid = josh_core::history::unapply_filter(
        transaction,
        filter,
        original_target,
        old_filtered_oid,
        local_commit,
        josh_core::history::OrphansMode::Keep,
        None,
    )
    .context("Failed to unapply filter")?;

    log::debug!("unfiltered_oid: {:?}", unfiltered_oid);

    let to_push = build_to_push(
        repo,
        &push_mode,
        remote_ref,
        remote_ref,
        unfiltered_oid,
        original_target,
    )
    .context("Failed to build to push")?;

    log::debug!("to_push: {:?}", to_push);

    let pr_infos = if matches!(push_mode, PushMode::Split(_)) && *forge == Some(Forge::Github) {
        josh_github_changes::collect_pr_infos(repo, &to_push)
    } else {
        vec![]
    };

    Ok(PreparedPush { to_push, pr_infos })
}

/// Push all refs to the remote in a single bundled `git push` invocation.
///
/// Every ref shares one remote URL and a uniform set of flags, so they are pushed together
/// rather than one process per ref. This also makes `--atomic` meaningful across the whole
/// set instead of applying to a single ref at a time.
fn push_refs(
    remote_name: &str,
    to_push: &[PushRef],
    repo_path: &std::path::Path,
    url: &str,
    force: bool,
    atomic: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    if to_push.is_empty() {
        return Ok(());
    }

    let mut git_push_args = vec!["push"];

    if force {
        git_push_args.push("--force");
    }

    if atomic {
        git_push_args.push("--atomic");
    }

    if dry_run {
        git_push_args.push("--dry-run");
    }

    git_push_args.push(url);

    for push_ref in to_push {
        eprintln!(
            "Pushing {} to {}/{}",
            push_ref.oid, remote_name, push_ref.ref_name
        );
    }

    let refspecs: Vec<String> = to_push
        .iter()
        .map(|push_ref| format!("{}:{}", push_ref.oid, push_ref.ref_name))
        .collect();
    git_push_args.extend(refspecs.iter().map(String::as_str));

    spawn_git_command(repo_path, &git_push_args, &[])
        .with_context(|| format!("Failed to push to {}", remote_name))?;

    eprintln!("Pushed {} ref(s) to {}", to_push.len(), remote_name);

    Ok(())
}

/// Create or update GitHub PRs for the collected push refs.
fn create_prs(
    pr_infos: &[josh_github_changes::PrInfo],
    url: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    if pr_infos.is_empty() {
        return Ok(());
    }

    use crate::forge::github;

    let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;

    if let Err(e) = rt.block_on(async {
        let api_connection = github::make_api_connection().await;
        let api_connection = api_connection.with_context(|| github::api_connection_hint())?;

        josh_github_changes::create_or_update_prs(&api_connection, url, pr_infos, dry_run).await
    }) {
        eprintln!("Warning: failed to create/update GitHub PRs: {}", e);
    }

    Ok(())
}

fn orchestrate_push(
    remote: Option<&str>,
    refspecs_arg: &[String],
    base: Option<&str>,
    force: bool,
    atomic: bool,
    dry_run: bool,
    push_mode: PushMode,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let remote_name = remote.unwrap_or("origin");

    let RemoteConfig {
        url,
        filter_with_meta,
        forge,
        ..
    } = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

    let filter = filter_with_meta.peel();

    let refspecs = if refspecs_arg.is_empty() {
        let head = repo.head().context("Failed to get HEAD")?;
        let current_branch = head
            .shorthand()
            .context("Failed to get current branch name")?;
        vec![current_branch.to_string()]
    } else {
        refspecs_arg.to_vec()
    };

    // Phase 1: Prepare all pushes (pure computation).
    let prepared_pushes: Vec<PreparedPush> = refspecs
        .iter()
        .map(|refspec| {
            prepare_push(
                refspec,
                remote_name,
                base,
                transaction,
                filter,
                &push_mode,
                &forge,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Phase 2: Flatten the prepared pushes into one bundled set. Dedup by destination ref
    // name (keep first) to tolerate duplicate or colliding refspec arguments, which an
    // atomic push would otherwise reject.
    let mut seen = std::collections::HashSet::new();
    let mut to_push: Vec<PushRef> = Vec::new();
    let mut pr_infos: Vec<josh_github_changes::PrInfo> = Vec::new();

    for prepared in prepared_pushes {
        for push_ref in prepared.to_push {
            if seen.insert(push_ref.ref_name.clone()) {
                to_push.push(push_ref);
            }
        }
        pr_infos.extend(prepared.pr_infos);
    }

    // Split mode always force-updates its per-change refs.
    let force = force || matches!(push_mode, PushMode::Split(_));

    // Phase 3: Execute the side effects.
    push_refs(
        remote_name,
        &to_push,
        repo.path(),
        &url,
        force,
        atomic,
        dry_run,
    )?;

    create_prs(&pr_infos, &url, dry_run)?;

    Ok(())
}

pub fn handle_push(
    args: &PushArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    orchestrate_push(
        args.remote.as_deref(),
        &args.refspecs,
        args.base.as_deref(),
        args.force,
        args.atomic,
        args.dry_run,
        PushMode::Normal,
        transaction,
    )
}

pub fn handle_publish(
    args: &PublishArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let config = repo.config().context("Failed to get git config")?;
    let push_mode = PushMode::Split(config.get_string("user.email").unwrap_or_default());

    orchestrate_push(
        args.remote.as_deref(),
        &args.refspecs,
        args.base.as_deref(),
        args.force,
        args.atomic,
        args.dry_run,
        push_mode,
        transaction,
    )
}
