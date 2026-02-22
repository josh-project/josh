use anyhow::{Context, anyhow};

use josh_changes::{PushMode, PushRef, build_to_push};
use josh_core::git::{normalize_repo_path, spawn_git_command};

use crate::config::{RemoteConfig, read_remote_config};
use crate::forge::Forge;

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
    author: &str,
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

    let mut changes: Option<Vec<josh_core::Change>> =
        if push_mode == PushMode::Stack || push_mode == PushMode::Split {
            Some(vec![])
        } else {
            None
        };

    let unfiltered_oid = josh_core::history::unapply_filter(
        transaction,
        filter,
        original_target,
        old_filtered_oid,
        local_commit,
        josh_core::history::OrphansMode::Keep,
        None,
        &mut changes,
    )
    .context("Failed to unapply filter")?;

    log::debug!("unfiltered_oid: {:?}", unfiltered_oid);

    let to_push = build_to_push(
        repo,
        changes,
        push_mode,
        remote_ref,
        author,
        remote_ref,
        unfiltered_oid,
        original_target,
    )
    .context("Failed to build to push")?;

    log::debug!("to_push: {:?}", to_push);

    let pr_infos = if !dry_run
        && (push_mode == PushMode::Split || push_mode == PushMode::Stack)
        && *forge == Some(Forge::Github)
    {
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

        if force || prepared.push_mode == PushMode::Split || prepared.push_mode == PushMode::Stack {
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

pub fn handle_push(
    args: &PushArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let repo_path = normalize_repo_path(repo.path());

    let remote_name = args.remote.as_deref().unwrap_or("origin");

    let RemoteConfig {
        url,
        filter_with_meta,
        forge,
        ..
    } = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;

    let filter = filter_with_meta.peel();

    let config = repo.config().context("Failed to get git config")?;
    let author = config.get_string("user.email").unwrap_or_default();
    let push_mode = args.push_mode.mode();

    let refspecs = if args.refspecs.is_empty() {
        let head = repo.head().context("Failed to get HEAD")?;
        let current_branch = head
            .shorthand()
            .context("Failed to get current branch name")?;
        vec![current_branch.to_string()]
    } else {
        args.refspecs.clone()
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
                push_mode,
                &author,
                &forge,
                args.dry_run,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Phase 2: Execute all pushes (side effects)
    for prepared in &prepared_pushes {
        execute_push(
            prepared,
            repo.path(),
            &url,
            args.force,
            args.atomic,
            args.dry_run,
        )?;
    }

    Ok(())
}
