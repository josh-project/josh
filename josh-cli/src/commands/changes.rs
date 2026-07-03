use std::collections::HashSet;

use anyhow::anyhow;

use crate::commands::scope::ScopeArgs;

/// Arguments for `josh changes list`.
#[derive(Debug, clap::Parser)]
pub struct ListArgs {
    #[command(flatten)]
    pub scope: ScopeArgs,
}

/// Arguments for `josh changes show`.
#[derive(Debug, clap::Parser)]
pub struct ShowArgs {
    /// Change-Id to display.
    #[arg()]
    pub change_id: String,

    #[command(flatten)]
    pub scope: ScopeArgs,
}

/// Arguments for `josh changes deps`.
#[derive(Debug, clap::Parser)]
pub struct DepsArgs {
    /// Change-Id whose dependencies to print.
    #[arg()]
    pub change_id: String,

    #[command(flatten)]
    pub scope: ScopeArgs,
}

/// Print one summary row per change stored on the resolved changes ref.
pub fn handle_list(
    args: &ListArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let scope = args.scope.resolve(repo)?;
    let changes = josh_changes::list_changes(repo, &scope)?;

    let scope_label = scope_label(&scope);

    if changes.is_empty() {
        println!("No changes found on {}.", scope_label);
        return Ok(());
    }

    let known = known_change_ids(&changes);

    struct Row {
        id: String,
        subject: String,
        deps_count: usize,
        comments_count: usize,
        vote: String,
    }

    let mut rows: Vec<Row> = Vec::with_capacity(changes.len());
    for change in &changes {
        let id = change.id().unwrap_or("<no-change-id>").to_string();
        let commit = repo.find_commit(change.commit())?;
        let subject = commit
            .message()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let deps_count = change
            .dependency_ids(repo, &known)
            .unwrap_or_default()
            .len();

        let comments_count = change
            .id()
            .map(|cid| {
                josh_changes::read_comments(repo, cid, &scope)
                    .map(|v| v.len())
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        let vote = change
            .id()
            .and_then(|cid| {
                josh_changes::read_vote(repo, cid, None, &scope)
                    .ok()
                    .flatten()
            })
            .map(|v| v.state)
            .unwrap_or_default();

        rows.push(Row {
            id,
            subject,
            deps_count,
            comments_count,
            vote,
        });
    }

    // Fewest deps last: sort by deps desc, tiebreak by subject for determinism.
    rows.sort_by(|a, b| {
        b.deps_count
            .cmp(&a.deps_count)
            .then_with(|| a.subject.cmp(&b.subject))
    });

    let id_w = rows.iter().map(|r| r.id.len()).max().unwrap_or(8);
    let vote_w = rows.iter().map(|r| r.vote.len()).max().unwrap_or(0).max(4);

    println!("Changes on {}:\n", scope_label);
    for r in &rows {
        println!(
            "{:<id_w$}  D={:>3}  C={:>3}  V={:<vote_w$}  {}",
            r.id,
            r.deps_count,
            r.comments_count,
            r.vote,
            r.subject,
            id_w = id_w,
            vote_w = vote_w,
        );
    }

    Ok(())
}

/// Print full detail for one change: metadata, commit message, file stats, comments.
pub fn handle_show(
    args: &ShowArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let scope = args.scope.resolve(repo)?;
    let change = resolve_change_by_id(repo, &scope, &args.change_id)?;

    let commit = repo.find_commit(change.commit())?;
    let msg = commit.message().unwrap_or("");
    let subject = msg.lines().next().unwrap_or("");
    let author = commit.author().email().unwrap_or("").to_string();
    let date = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();

    println!("Change-Id: {}", args.change_id);
    println!("Commit:    {}", change.commit());
    println!("Author:    {}", author);
    println!("Date:      {}", date);
    let series = change.series().join(", ");
    if !series.is_empty() {
        println!("Series:    {}", series);
    }

    if let Ok(Some(json)) = josh_changes::read_pr_data(repo, &args.change_id, &scope) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
            let url = v["url"].as_str().unwrap_or("");
            let title = v["title"].as_str().unwrap_or("");
            let state = v["state"].as_str().unwrap_or("");
            let rd = v["review_decision"].as_str().unwrap_or("");
            print!("PR:        {} [{}]", title, state);
            if !rd.is_empty() {
                print!(" {}", rd);
            }
            if !url.is_empty() {
                print!(" {}", url);
            }
            println!();
        }
    }

    if let Some(vote) = josh_changes::read_vote(repo, &args.change_id, None, &scope)?
        .map(|v| v.state)
        .filter(|s| !s.is_empty())
    {
        println!("Vote:      {}", vote);
    }

    println!();
    println!("Subject:   {}", subject);

    println!();
    let files = file_stats(repo, &commit)?;
    let total_adds: usize = files.iter().map(|f| f.adds).sum();
    let total_dels: usize = files.iter().map(|f| f.dels).sum();
    println!(
        "Files ({}, +{} / -{}):",
        files.len(),
        total_adds,
        total_dels
    );
    for f in &files {
        println!("  +{:<4} -{:<4}  {}", f.adds, f.dels, f.path);
    }

    let comments = josh_changes::read_comments(repo, &args.change_id, &scope).unwrap_or_default();
    println!();
    println!("Comments ({}):", comments.len());
    if comments.is_empty() {
        return Ok(());
    }
    print_comment_threads(&comments);

    Ok(())
}

/// Print just the change-ids this change depends on (stored changes whose
/// commits appear in this change's first-parent walk to base).
pub fn handle_deps(
    args: &DepsArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let scope = args.scope.resolve(repo)?;
    let changes = josh_changes::list_changes(repo, &scope)?;
    let known = known_change_ids(&changes);

    let change = changes
        .iter()
        .find(|c| c.id() == Some(args.change_id.as_str()))
        .ok_or_else(|| {
            anyhow!(
                "change-id '{}' not found on {}",
                args.change_id,
                scope_label(&scope)
            )
        })?;

    let mut deps: Vec<(String, String)> = Vec::new();
    for dep_id in change.dependency_ids(repo, &known)? {
        let subject = changes
            .iter()
            .find(|c| c.id() == Some(dep_id.as_str()))
            .and_then(|c| repo.find_commit(c.commit()).ok())
            .and_then(|c| {
                c.message()
                    .map(|m| m.lines().next().unwrap_or("").to_string())
            })
            .unwrap_or_default();
        deps.push((dep_id, subject));
    }

    if deps.is_empty() {
        println!("{} has no dependencies on stored changes.", args.change_id);
        return Ok(());
    }

    let w = deps.iter().map(|(id, _)| id.len()).max().unwrap_or(0);
    println!("Depends on:");
    for (id, subject) in &deps {
        println!("  {:<w$}  {}", id, subject, w = w);
    }
    Ok(())
}

fn scope_label(scope: &josh_changes::ChangesRef) -> String {
    match scope {
        josh_changes::ChangesRef::Local { branch } => format!("Local [{}]", branch),
        josh_changes::ChangesRef::Remote { remote, branch } => {
            format!("remote '{}' [{}]", remote, branch)
        }
    }
}

fn known_change_ids(changes: &[josh_changes::Change]) -> HashSet<String> {
    changes
        .iter()
        .filter_map(|c| c.id().map(|s| s.to_string()))
        .collect()
}

fn resolve_change_by_id(
    repo: &git2::Repository,
    scope: &josh_changes::ChangesRef,
    change_id: &str,
) -> anyhow::Result<josh_changes::Change> {
    josh_changes::list_changes(repo, scope)?
        .into_iter()
        .find(|c| c.id() == Some(change_id))
        .ok_or_else(|| {
            anyhow!(
                "change-id '{}' not found on {}",
                change_id,
                scope_label(scope)
            )
        })
}

struct FileStat {
    path: String,
    adds: usize,
    dels: usize,
}

fn file_stats(repo: &git2::Repository, commit: &git2::Commit) -> anyhow::Result<Vec<FileStat>> {
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;
    let mut files = Vec::with_capacity(diff.deltas().len());
    for i in 0..diff.deltas().len() {
        let delta = diff.deltas().nth(i).unwrap();
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let patch = git2::Patch::from_diff(&diff, i)?;
        let (_, adds, dels) = patch
            .as_ref()
            .map(|p| p.line_stats().unwrap_or((0, 0, 0)))
            .unwrap_or((0, 0, 0));
        files.push(FileStat { path, adds, dels });
    }
    Ok(files)
}

fn print_comment_threads(comments: &[josh_changes::Comment]) {
    // Sort top-level roots by timestamp asc.
    let mut roots: Vec<usize> = comments
        .iter()
        .enumerate()
        .filter(|(_, c)| c.reply_to.is_none())
        .map(|(i, _)| i)
        .collect();
    roots.sort_by(|&a, &b| {
        comments[a]
            .timestamp
            .as_deref()
            .unwrap_or("")
            .cmp(comments[b].timestamp.as_deref().unwrap_or(""))
    });
    for root in roots {
        print_comment(comments, root, 0);
    }
}

fn print_comment(comments: &[josh_changes::Comment], idx: usize, depth: usize) {
    let c = &comments[idx];
    let indent = "  ".repeat(depth + 1);
    let author = c.author.as_deref().unwrap_or("?");
    let ts = c
        .timestamp
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let loc = c
        .file
        .as_deref()
        .map(|f| {
            let line = c
                .location
                .as_ref()
                .map(|l| format!(":{}", l.start_line))
                .unwrap_or_default();
            format!("  ({}{})", f, line)
        })
        .unwrap_or_default();
    println!("{}[{}] {}{}", indent, author, ts, loc);
    for line in c.message.lines() {
        println!("{}  {}", indent, line);
    }
    let children: Vec<usize> = comments
        .iter()
        .enumerate()
        .filter(|(_, child)| child.reply_to.as_deref() == Some(&c.id))
        .map(|(i, _)| i)
        .collect();
    for child in children {
        print_comment(comments, child, depth + 1);
    }
}
