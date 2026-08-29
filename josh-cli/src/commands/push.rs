use anyhow::{Context, anyhow};

use josh_changes::{PushMode, PushRef, StackedChangeRef, StackedRef, build_to_push};
use josh_core::git::normalize_repo_path;

use crate::config::{RemoteConfig, read_remote_config};
use crate::forge::{Forge, GerritMode};
use crate::porcelain::PushRefUpdate;

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

    /// Wrap the reverse-filtered commit in a merge commit on top of the
    /// base (mirrors `josh-proxy`'s `git push -o merge`).
    ///
    /// The resulting commit has two parents — the base and the
    /// reverse-filtered new commit — and its tree is the 3-way merge of
    /// both. Requires --base, or a destination branch that already
    /// exists on the remote, to anchor the merge.
    #[arg(long = "merge", action = clap::ArgAction::SetTrue)]
    pub merge: bool,
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

    /// Wrap the reverse-filtered commit in a merge commit on top of the
    /// base.
    ///
    /// See `josh push --merge` for details.
    #[arg(long = "merge", action = clap::ArgAction::SetTrue)]
    pub merge: bool,
}

struct PreparedPush {
    to_push: Vec<PushRef>,
    pr_infos: Vec<josh_github_changes::PrInfo>,
}

fn prepare_push(
    refspec: &str,
    remote_name: &str,
    base: Option<&str>,
    merge: bool,
    transaction: &josh_core::cache::Transaction,
    filter: josh_core::filter::Filter,
    push_mode: &PushMode,
    forge: &Option<Forge>,
    gerrit_mode: GerritMode,
    dry_run: bool,
) -> anyhow::Result<PreparedPush> {
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

    let local_ref_name = transaction
        .expand_ref_name(&local_ref)?
        .with_context(|| format!("Failed to resolve local ref '{}'", local_ref))?;
    let local_commit = transaction
        .resolve_ref(&local_ref_name)?
        .context("Failed to get target of local ref")?;

    let dest_remote_ref = format!("refs/josh/remotes/{}/{}", remote_name, remote_ref);
    let (dest_oid, old_filtered_oid) =
        if let Some(dest_oid) = transaction.resolve_ref(&dest_remote_ref)? {
            let (filtered_oids, errors) =
                josh_core::filter_refs(transaction, filter, &[(dest_remote_ref.clone(), dest_oid)]);

            if let Some(error) = errors.into_iter().next() {
                return Err(anyhow!("josh filter error: {}", error.1));
            }

            let old_filtered = if let Some((_, filtered_oid)) = filtered_oids.first() {
                *filtered_oid
            } else {
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1)
            };

            (dest_oid, old_filtered)
        } else {
            (
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            )
        };

    let original_target = if let Some(base) = base {
        let base_remote_ref = format!("refs/josh/remotes/{}/{}", remote_name, base);
        transaction
            .resolve_ref(&base_remote_ref)?
            .ok_or_else(|| anyhow!("no such ref: '{}'", base_remote_ref))
            .with_context(|| {
                format!(
                    "Failed to resolve --base ref (looked up '{}')",
                    base_remote_ref
                )
            })?
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
        base.map(|_| original_target),
    )
    .context("Failed to unapply filter")?;

    let unfiltered_oid = if merge {
        if original_target == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
            return Err(anyhow!(
                "--merge requires --base=<ref> or an existing destination ref"
            ));
        }
        let odb = transaction.odb();
        let merged_tree =
            josh_core::objects::merge_commits(odb, original_target, unfiltered_oid, None)?;
        let signature = josh_core::git::josh_actor_signature()?;
        josh_core::objects::write_commit(
            odb,
            merged_tree,
            &[original_target, unfiltered_oid],
            &signature,
            &signature,
            &format!("Merge from {}", josh_core::filter::pretty(filter, 0)),
        )?
    } else {
        unfiltered_oid
    };

    log::debug!("unfiltered_oid: {:?}", unfiltered_oid);

    // Gerrit publishing pushes to the magic ref `refs/for/<branch>` instead of
    // josh's `@changes`/`@base` ref pairs, and needs no PR API call. The mode
    // decides the mapping: `independent` (default) pushes only dependency-free
    // changes as separate reviews; `stack` pushes the whole history as one
    // relation chain.
    let to_push = match (forge, push_mode) {
        (Some(Forge::Gerrit), PushMode::Publish(_)) => match gerrit_mode {
            GerritMode::Independent => josh_gerrit_changes::build_gerrit_independent_push(
                transaction,
                remote_ref,
                unfiltered_oid,
                original_target,
            )
            .context("Failed to build Gerrit push")?,
            GerritMode::Stack => josh_gerrit_changes::build_gerrit_push(
                transaction,
                remote_ref,
                unfiltered_oid,
                original_target,
            )
            .context("Failed to build Gerrit push")?,
        },
        _ => build_to_push(
            transaction,
            push_mode,
            remote_ref,
            remote_ref,
            unfiltered_oid,
            original_target,
        )
        .context("Failed to build to push")?,
    };

    log::debug!("to_push: {:?}", to_push);

    let pr_infos =
        if !dry_run && matches!(push_mode, PushMode::Publish(_)) && *forge == Some(Forge::Github) {
            josh_github_changes::collect_pr_infos(transaction, &to_push)
        } else {
            vec![]
        };

    Ok(PreparedPush { to_push, pr_infos })
}

/// Render a curated summary of a push, reframing the stacked-changes refs
/// (`@changes/`) as a change count and hiding the internal `@base/`/`@heads/`
/// refs entirely. Rejected updates are an error (git's stderr, carrying the
/// rejection details, has already been forwarded to the user).
pub fn render_push_summary(
    updates: &[PushRefUpdate],
    dry_run: bool,
) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut new_changes = 0usize;
    let mut updated_changes = 0usize;

    for update in updates {
        if let PushRefUpdate::Rejected { reference, reason } = update {
            anyhow::bail!(
                "git push rejected update to {}{}",
                reference,
                reason
                    .as_deref()
                    .map(|r| format!(" ({})", r))
                    .unwrap_or_default()
            );
        }

        let Some(name) = update.reference().strip_prefix("refs/heads/") else {
            continue;
        };

        match StackedRef::parse(name) {
            Some(StackedRef::ChangeRef(StackedChangeRef::Change { .. })) => {
                match update {
                    PushRefUpdate::New { .. } => new_changes += 1,
                    PushRefUpdate::FastForward { .. } | PushRefUpdate::Forced { .. } => {
                        updated_changes += 1
                    }
                    _ => {}
                }
                continue;
            }
            // @base and @heads refs are internal plumbing; hide them entirely.
            Some(_) => continue,
            None => {}
        }

        let line = match update {
            PushRefUpdate::New { .. } => format!("new branch {}", name),
            PushRefUpdate::Deleted { .. } => format!("deleted branch {}", name),
            PushRefUpdate::Forced { old, new, .. } => {
                format!("force-updated {} ({}..{})", name, old, new)
            }
            PushRefUpdate::FastForward { old, new, .. } => {
                format!("updated {} ({}..{})", name, old, new)
            }
            PushRefUpdate::UpToDate { .. } => continue,
            PushRefUpdate::Rejected { .. } => unreachable!("rejected updates bail out above"),
        };
        lines.push(line);
    }

    let total_changes = new_changes + updated_changes;
    if total_changes > 0 {
        lines.push(format!(
            "{} {} change{}{}",
            if dry_run {
                "Would publish"
            } else {
                "published"
            },
            total_changes,
            if total_changes == 1 { "" } else { "s" },
            if new_changes > 0 {
                format!(" ({} new)", new_changes)
            } else {
                String::new()
            },
        ));
    } else if lines.is_empty() {
        lines.push("Everything up-to-date".to_string());
    }

    Ok(lines)
}

/// Push each change to a change-based forge (Gerrit), one `git push` per ref.
///
/// Every change targets the same magic ref `refs/for/<branch>`, so a single
/// bundled push (multiple source commits, one destination ref) is not
/// possible -- each ref goes in its own invocation. `refs/for` refs are never
/// force-pushed; the forge keys patchsets by Change-Id instead.
fn push_change_based(
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
    to_push: &[PushRef],
    url: &str,
    atomic: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    if to_push.is_empty() {
        return Ok(());
    }

    for push_ref in to_push {
        eprintln!(
            "Pushing {} to {}/{}",
            push_ref.oid, remote_name, push_ref.ref_name
        );
        let mut git_push_args = vec!["push"];
        if atomic {
            git_push_args.push("--atomic");
        }
        if dry_run {
            git_push_args.push("--dry-run");
        }
        git_push_args.push(url);
        let refspec = format!("{}:{}", push_ref.oid, push_ref.ref_name);
        git_push_args.push(&refspec);
        transaction
            .spawn_git(&git_push_args, &[])
            .with_context(|| format!("Failed to push to {}", remote_name))?;
    }
    eprintln!("Pushed {} ref(s) to {}", to_push.len(), remote_name);
    Ok(())
}

/// Push all refs to a branch-based forge in a single bundled `git push` invocation.
///
/// Every ref shares one remote URL and a uniform set of flags, so they are pushed together
/// rather than one process per ref. This also makes `--atomic` meaningful across the whole
/// set instead of applying to a single ref at a time.
///
/// In curated mode (stacked-changes publish) the push runs with `--porcelain`:
/// stdout is captured and rendered as a summary via `render_push_summary`,
/// while stderr keeps the default handling (inherited on a TTY, forwarded
/// otherwise) so progress/errors reach the user.
fn push_branch_based(
    transaction: &josh_core::cache::Transaction,
    remote_name: &str,
    to_push: &[PushRef],
    url: &str,
    force: bool,
    atomic: bool,
    dry_run: bool,
    curated: bool,
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

    if curated {
        git_push_args.push("--porcelain");
    }

    git_push_args.push(url);

    if !curated {
        for push_ref in to_push {
            eprintln!(
                "Pushing {} to {}/{}",
                push_ref.oid, remote_name, push_ref.ref_name
            );
        }
    }

    let refspecs: Vec<String> = to_push
        .iter()
        .map(|push_ref| format!("{}:{}", push_ref.oid, push_ref.ref_name))
        .collect();
    git_push_args.extend(refspecs.iter().map(String::as_str));

    if curated {
        let output = transaction
            .git_command(&git_push_args, &[])?
            .with_stdout(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to push to {}", remote_name))?;

        let updates =
            crate::porcelain::parse_push_porcelain(&String::from_utf8_lossy(&output.stdout))?;
        for line in render_push_summary(&updates, dry_run)? {
            eprintln!("{}", line);
        }
    } else {
        transaction
            .spawn_git(&git_push_args, &[])
            .with_context(|| format!("Failed to push to {}", remote_name))?;

        eprintln!("Pushed {} ref(s) to {}", to_push.len(), remote_name);
    }

    Ok(())
}

/// Create or update GitHub PRs for the collected push refs.
///
/// `url` is the PR target (upstream). `fork_url`, when set, is the repo the
/// change branches were pushed to; PRs are then opened with a cross-fork head.
fn create_prs(
    pr_infos: &[josh_github_changes::PrInfo],
    url: &str,
    fork_url: Option<&str>,
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

        josh_github_changes::create_or_update_prs(&api_connection, url, fork_url, pr_infos, dry_run)
            .await
    }) {
        eprintln!("Warning: failed to create/update GitHub PRs: {}", e);
    }

    Ok(())
}

fn orchestrate_push(
    remote: Option<&str>,
    refspecs_arg: &[String],
    base: Option<&str>,
    merge: bool,
    force: bool,
    atomic: bool,
    dry_run: bool,
    push_mode: PushMode,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo_path = normalize_repo_path(transaction.path());

    let remote_name = remote.unwrap_or("origin");

    let config = read_remote_config(&repo_path, remote_name)
        .with_context(|| format!("Failed to read remote config for '{}'", remote_name))?;
    let filter = config.semantic_filter();
    let RemoteConfig {
        url,
        forge,
        push_url,
        gerrit_mode,
        ..
    } = config;

    // Branches go to the fork (push_url) when configured; otherwise to `url`.
    // PRs are always opened against `url`.
    let push_target = push_url.as_deref().unwrap_or(&url);

    let refspecs = if refspecs_arg.is_empty() {
        let head = transaction.head().context("Failed to get HEAD")?;
        let current_branch = head
            .short_branch()
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
                merge,
                transaction,
                filter,
                &push_mode,
                &forge,
                gerrit_mode,
                dry_run,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Phase 2: Flatten the prepared pushes into one bundled set. Dedup by
    // (destination ref, oid) (keep first) to tolerate duplicate or colliding
    // refspec arguments, which an atomic push would otherwise reject. The oid is
    // part of the key because Gerrit publishing intentionally routes several
    // distinct change commits to the same `refs/for/<branch>` ref.
    let mut seen = std::collections::HashSet::new();
    let mut to_push: Vec<PushRef> = Vec::new();
    let mut pr_infos: Vec<josh_github_changes::PrInfo> = Vec::new();

    for prepared in prepared_pushes {
        for push_ref in prepared.to_push {
            if seen.insert((push_ref.ref_name.clone(), push_ref.oid)) {
                to_push.push(push_ref);
            }
        }
        pr_infos.extend(prepared.pr_infos);
    }

    // Publish mode always force-updates its per-change refs.
    let force = force || matches!(push_mode, PushMode::Publish(_));

    // Publish output is curated (porcelain + summary); plain push keeps raw
    // git output.
    let curated = matches!(push_mode, PushMode::Publish(_));

    // Phase 3: Execute the side effects. Publishing to a change-based forge
    // (Gerrit) pushes magic `refs/for/<branch>` refs; everything else is a
    // branch push. This mirrors the dispatch in `prepare_push`.
    if forge == Some(Forge::Gerrit) && matches!(push_mode, PushMode::Publish(_)) {
        push_change_based(
            transaction,
            remote_name,
            &to_push,
            push_target,
            atomic,
            dry_run,
        )?;
    } else {
        push_branch_based(
            transaction,
            remote_name,
            &to_push,
            push_target,
            force,
            atomic,
            dry_run,
            curated,
        )?;
    }

    create_prs(&pr_infos, &url, push_url.as_deref(), dry_run)?;

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
        args.merge,
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
    let push_mode = PushMode::Publish(transaction.config_string("user.email")?.unwrap_or_default());

    orchestrate_push(
        args.remote.as_deref(),
        &args.refspecs,
        args.base.as_deref(),
        args.merge,
        args.force,
        args.atomic,
        args.dry_run,
        push_mode,
        transaction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_ref(reference: &str) -> PushRefUpdate {
        PushRefUpdate::New {
            reference: reference.to_string(),
        }
    }

    fn forced(reference: &str) -> PushRefUpdate {
        PushRefUpdate::Forced {
            old: "1234567".to_string(),
            new: "abcdef1".to_string(),
            reference: reference.to_string(),
        }
    }

    #[test]
    fn summary_counts_changes_and_hides_internal_refs() {
        let updates = vec![
            new_ref("refs/heads/@changes/master/a@b.c/1234"),
            forced("refs/heads/@changes/master/a@b.c/5678"),
            new_ref("refs/heads/@base/master/a@b.c/1234"),
            forced("refs/heads/@heads/master/a@b.c"),
        ];

        let lines = render_push_summary(&updates, false).unwrap();

        assert_eq!(lines, vec!["published 2 changes (1 new)"]);
    }

    #[test]
    fn summary_renders_branch_updates() {
        let updates = vec![
            new_ref("refs/heads/feature"),
            PushRefUpdate::FastForward {
                old: "1234567".to_string(),
                new: "abcdef1".to_string(),
                reference: "refs/heads/master".to_string(),
            },
        ];

        let lines = render_push_summary(&updates, false).unwrap();

        assert_eq!(
            lines,
            vec!["new branch feature", "updated master (1234567..abcdef1)"]
        );
    }

    #[test]
    fn summary_up_to_date_republish_is_silent() {
        let updates = vec![
            PushRefUpdate::UpToDate {
                reference: "refs/heads/@changes/master/a@b.c/1234".to_string(),
            },
            PushRefUpdate::UpToDate {
                reference: "refs/heads/@heads/master/a@b.c".to_string(),
            },
        ];

        let lines = render_push_summary(&updates, false).unwrap();

        assert_eq!(lines, vec!["Everything up-to-date"]);
    }

    #[test]
    fn summary_dry_run_prefixes_would() {
        let updates = vec![new_ref("refs/heads/@changes/master/a@b.c/1234")];

        let lines = render_push_summary(&updates, true).unwrap();

        assert_eq!(lines, vec!["Would publish 1 change (1 new)"]);
    }

    #[test]
    fn summary_rejected_update_errors() {
        let updates = vec![PushRefUpdate::Rejected {
            reference: "refs/heads/@changes/master/a@b.c/1234".to_string(),
            reason: Some("non-fast-forward".to_string()),
        }];

        assert!(render_push_summary(&updates, false).is_err());
    }
}
