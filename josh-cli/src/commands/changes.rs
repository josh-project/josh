use crate::cli_println as println;

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

    let scopes = josh_changes::refs_on_branch(repo, &branch)?;
    let changes = josh_changes::list_changes_in_scopes(repo, &scopes)?;

    if changes.is_empty() {
        crate::output::set_data_value(serde_json::json!({
            "branch": branch,
            "changes": [],
        }));
        println!("No local changes found for branch '{}'.", branch);
        return Ok(());
    }

    println!("Changes targeting '{}':\n", branch);
    let mut output_changes = Vec::new();

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
        let mut output_commits = Vec::new();
        for oid in &contributing {
            if let Ok(c) = repo.find_commit(*oid) {
                let c_subject = c.message().unwrap_or("").lines().next().unwrap_or("");
                let c_short = &oid.to_string()[..8];
                println!("  {}  {}", c_short, c_subject);
                output_commits.push(serde_json::json!({
                    "oid": oid.to_string(),
                    "subject": c_subject,
                }));
            }
        }

        let comments = change
            .id()
            .map(|cid| {
                josh_changes::read_comments_in_scopes(repo, cid, &scopes).unwrap_or_default()
            })
            .unwrap_or_default();
        let mut output_comments = Vec::new();
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
                let location = if file.is_empty() {
                    String::new()
                } else {
                    format!(" ({}{})", file, line)
                };
                println!(
                    "    {} {}{}",
                    cid,
                    c.message.lines().next().unwrap_or(""),
                    location
                );
                output_comments.push(serde_json::json!({
                    "id": c.id,
                    "message": c.message,
                    "file": c.file,
                    "location": c.location.as_ref().map(|location| serde_json::json!({
                        "start_line": location.start_line,
                        "end_line": location.end_line,
                        "start_column": location.start_col,
                        "end_column": location.end_col,
                    })),
                }));
            }
        }

        output_changes.push(serde_json::json!({
            "id": change.id(),
            "subject": subject,
            "series": change.series(),
            "author": change.author(),
            "commit": change.commit().to_string(),
            "contributing_commits": output_commits,
            "comments": output_comments,
        }));
        println!();
    }

    crate::output::set_data_value(serde_json::json!({
        "branch": branch,
        "changes": output_changes,
    }));
    Ok(())
}
