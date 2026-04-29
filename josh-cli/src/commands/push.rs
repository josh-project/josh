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

    /// Delete remote @changes and @base branches for the current user that have no open PR
    #[arg(long = "prune-branches", action = clap::ArgAction::SetTrue)]
    pub prune_branches: bool,
}

struct PreparedPush {
    remote_name: String,
    to_push: Vec<PushRef>,
    push_mode: PushMode,
    pr_infos: Vec<josh_github_changes::PrInfo>,
}

fn prepare_push(
    refspec: &str,
    remote_name: &str,
    transaction: &josh_core::cache::Transaction,
    filter: josh_core::filter::Filter,
    push_mode: PushMode,
    forge: &Option<Forge>,
    dry_run: bool,
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

    // Look up the josh remote reference once and derive both original_target
    // and old_filtered_oid from it
    let josh_remote_ref = format!("refs/josh/remotes/{}/{}", remote_name, remote_ref);
    let (original_target, old_filtered_oid) =
        if let Ok(remote_reference) = repo.find_reference(&josh_remote_ref) {
            let josh_remote_oid = remote_reference.target().unwrap_or(git2::Oid::zero());

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

    let pr_infos =
        if !dry_run && matches!(push_mode, PushMode::Publish(_)) && *forge == Some(Forge::Github) {
            josh_github_changes::collect_pr_infos(repo, &to_push)
        } else {
            vec![]
        };

    Ok(PreparedPush {
        remote_name: remote_name.to_string(),
        to_push,
        push_mode,
        pr_infos,
    })
}

fn execute_push(
    prepared: &PreparedPush,
    repo_path: &std::path::Path,
    url: &str,
    force: bool,
    atomic: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    for push_ref in &prepared.to_push {
        let mut git_push_args = vec!["push"];

        if force || matches!(prepared.push_mode, PushMode::Publish(_)) {
            git_push_args.push("--force");
        }

        if atomic {
            git_push_args.push("--atomic");
        }

        if dry_run {
            git_push_args.push("--dry-run");
        }

        let target_remote = url.to_string();
        let push_refspec = format!("{}:{}", push_ref.oid, push_ref.ref_name);

        git_push_args.push(&target_remote);
        git_push_args.push(&push_refspec);

        if let Err(e) = spawn_git_command(repo_path, &git_push_args, &[]) {
            eprintln!(
                "Failed to push {} to {}/{}",
                push_ref.oid, prepared.remote_name, push_ref.ref_name
            );
            eprintln!("{}", e);
        } else {
            eprintln!(
                "Pushed {} to {}/{}",
                push_ref.oid, prepared.remote_name, push_ref.ref_name
            );
        }
    }

    if !prepared.pr_infos.is_empty() {
        use crate::forge::github;

        let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;

        if let Err(e) = rt.block_on(async {
            let api_connection = github::make_api_connection().await;
            let api_connection = api_connection.with_context(|| github::api_connection_hint())?;

            josh_github_changes::create_or_update_prs(&api_connection, url, &prepared.pr_infos)
                .await
        }) {
            eprintln!("Warning: failed to create/update GitHub PRs: {}", e);
        }
    }

    Ok(())
}

fn run_push(
    remote: Option<&str>,
    refspecs_arg: &[String],
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

    // Phase 1: Prepare all pushes (pure computation)
    let prepared_pushes: Vec<PreparedPush> = refspecs
        .iter()
        .map(|refspec| {
            prepare_push(
                refspec,
                remote_name,
                transaction,
                filter,
                push_mode.clone(),
                &forge,
                dry_run,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Phase 2: Execute all pushes (side effects)
    for prepared in &prepared_pushes {
        execute_push(prepared, repo.path(), &url, force, atomic, dry_run)?;
    }

    Ok(())
}

pub fn handle_push(
    args: &PushArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    run_push(
        args.remote.as_deref(),
        &args.refspecs,
        args.force,
        args.atomic,
        args.dry_run,
        PushMode::Normal,
        transaction,
    )
}

fn run_prune_branches(
    remote: Option<&str>,
    dry_run: bool,
    email: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let remote_name = remote.unwrap_or("origin");

    let RemoteConfig { url, forge, .. } = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

    if forge != Some(Forge::Github) {
        return Ok(());
    }

    let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;

    if let Err(e) = rt.block_on(async {
        use crate::forge::github;

        let api_connection = github::make_api_connection().await;
        let api_connection = api_connection.with_context(|| github::api_connection_hint())?;

        josh_github_changes::prune_stale_branches(&api_connection, &url, email, dry_run).await
    }) {
        eprintln!("Warning: failed to prune branches: {}", e);
    }

    Ok(())
}

pub fn handle_publish(
    args: &PublishArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let config = repo.config().context("Failed to get git config")?;
    let email = config.get_string("user.email").unwrap_or_default();
    let push_mode = PushMode::Publish(email.clone());

    run_push(
        args.remote.as_deref(),
        &args.refspecs,
        args.force,
        args.atomic,
        args.dry_run,
        push_mode,
        transaction,
    )?;

    if args.prune_branches {
        let remote_name = args.remote.as_deref().unwrap_or("origin");
        prune_local_change_branches(remote_name, transaction)?;
        run_prune_branches(args.remote.as_deref(), args.dry_run, &email, transaction)?;
    }

    Ok(())
}

fn prune_local_change_branches(
    remote_name: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let prefixes = [
        format!("refs/josh/remotes/{}/@changes/", remote_name),
        format!("refs/josh/remotes/{}/@base/", remote_name),
        format!("refs/namespaces/josh-{}/refs/heads/@changes/", remote_name),
        format!("refs/namespaces/josh-{}/refs/heads/@base/", remote_name),
    ];

    for prefix in &prefixes {
        let refs: Vec<String> = repo
            .references_glob(&format!("{}*", prefix))?
            .filter_map(|r| r.ok())
            .filter_map(|r| r.name().map(String::from))
            .collect();

        for ref_name in refs {
            if let Err(e) = repo.find_reference(&ref_name).and_then(|mut r| r.delete()) {
                eprintln!("Warning: failed to delete {}: {}", ref_name, e);
            }
        }
    }

    Ok(())
}
