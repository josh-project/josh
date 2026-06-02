/// Arguments for `josh changes list`.
#[derive(Debug, clap::Parser)]
pub struct ListArgs {
    /// Target branch to list changes for (defaults to HEAD's branch).
    #[arg(short = 'b', long = "branch")]
    pub branch: Option<String>,
}

/// Print changes read from refs/josh/changes/<branch> + refs/josh/remotes/*/changes/<branch>
/// (populated by `josh changes sync`).
pub fn handle_list(
    args: &ListArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => josh_changes::head_branch(repo)?,
    };

    let changes = josh_changes::list_changes_on_branch(repo, &branch)?;

    if changes.is_empty() {
        println!("No local changes found for branch '{}'.", branch);
        return Ok(());
    }

    println!("Changes targeting '{}':\n", branch);

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

        let comments = change
            .id()
            .map(|cid| {
                josh_changes::read_comments_on_branch(repo, cid, &branch).unwrap_or_default()
            })
            .unwrap_or_default();
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
