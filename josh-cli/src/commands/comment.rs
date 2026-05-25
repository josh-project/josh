use anyhow::{Context, anyhow};

/// Arguments for `josh changes comment`.
#[derive(Debug, clap::Parser)]
pub struct CommentArgs {
    /// Change to comment on (Change-Id, ref, or SHA).
    #[arg()]
    pub change: String,

    /// Comment message.
    #[arg(short = 'm', long = "message")]
    pub message: String,
}

pub fn handle_comment(
    args: &CommentArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let oid = resolve_change(repo, &args.change)?;
    josh_changes::write_comment(repo, oid, &args.message)?;

    println!("Comment saved.");
    Ok(())
}

fn resolve_change(repo: &git2::Repository, spec: &str) -> anyhow::Result<git2::Oid> {
    // Try as a full OID first.
    if let Ok(oid) = git2::Oid::from_str(spec) {
        if repo.find_commit(oid).is_ok() {
            return Ok(oid);
        }
    }

    // Try as a revparse (branch, tag, short SHA).
    if let Ok(obj) = repo.revparse_single(spec) {
        if let Ok(commit) = obj.peel_to_commit() {
            return Ok(commit.id());
        }
    }

    // Walk HEAD to find a commit with matching Change-Id.
    let head = repo.head().context("no HEAD; cannot search by Change-Id")?;
    let tip = head.peel_to_commit()?.id();
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(tip)?;
    for oid in walk {
        let oid = oid?;
        if let Ok(c) = repo.find_commit(oid) {
            let (id, _) = josh_changes::parse_change_meta(c.message().unwrap_or(""));
            if id.as_deref() == Some(spec) {
                return Ok(oid);
            }
        }
    }

    Err(anyhow!("could not resolve '{}' to a commit", spec))
}
