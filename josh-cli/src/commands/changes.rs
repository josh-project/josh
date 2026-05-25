use anyhow::Context;

/// Arguments for `josh changes list`.
#[derive(Debug, clap::Parser)]
pub struct ListArgs {
    /// Override the base ref (default: origin/<current-branch>).
    #[arg(short = 'b', long = "base")]
    pub base: Option<String>,
}

/// Print downstacked changes between the current branch tip and its remote
/// tracking branch.  All authors are included (no author filtering).
pub fn handle_list(
    args: &ListArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Determine tip and branch name from HEAD.
    let head = repo.head().context("Failed to get HEAD")?;
    let branch = head
        .shorthand()
        .context("Detached HEAD -- cannot determine current branch")?;

    let tip = head.peel_to_commit().context("HEAD has no target")?.id();

    // Resolve base -- either an explicit --base argument or origin/<branch>.
    let base = if let Some(base_ref) = &args.base {
        repo.find_reference(base_ref)
            .with_context(|| format!("base ref '{}' not found", base_ref))?
            .peel_to_commit()
            .context("base ref has no target")?
            .id()
    } else {
        let remote_ref = format!("refs/remotes/origin/{}", branch);
        repo.find_reference(&remote_ref)
            .with_context(|| {
                format!(
                    "no remote tracking branch '{}' found -- \
                     has this branch been pushed?\n\
                     Use --base to specify a base ref",
                    remote_ref,
                )
            })?
            .peel_to_commit()
            .context("remote tracking ref has no target")?
            .id()
    };

    let changes = josh_changes::list_changes(repo, tip, base)?;

    if changes.is_empty() {
        println!("No local changes found.");
        return Ok(());
    }

    for change in &changes {
        let commit = repo.find_commit(change.commit())?;
        let subject = commit.message().unwrap_or("").lines().next().unwrap_or("");

        let id = change.id().unwrap_or("<no-change-id>");
        let series = if change.series().is_empty() {
            String::new()
        } else {
            format!(" [{}]", change.series().join(", "))
        };

        println!("{} {}{} ({})", id, subject, series, change.author());

        let contributing = change.contributing(repo)?;
        for oid in &contributing {
            if let Ok(c) = repo.find_commit(*oid) {
                let c_subject = c.message().unwrap_or("").lines().next().unwrap_or("");
                let c_short = &oid.to_string()[..8];
                println!("  {}  {}", c_short, c_subject);
            }
        }

        let comments = josh_changes::read_comments(repo, change).unwrap_or_default();
        if !comments.is_empty() {
            println!();
            for c in &comments {
                let cid = &c.id[..8.min(c.id.len())];
                let file = c.file.as_deref().unwrap_or("");
                let line = c
                    .location
                    .as_ref()
                    .map(|l| format!(":{}", l.start_line))
                    .unwrap_or_default();
                print!("    {} {}", cid, c.message.lines().next().unwrap_or(""));
                if !file.is_empty() {
                    print!(" ({}{})", file, line);
                }
                println!();
            }
        }

        println!();
    }

    Ok(())
}
