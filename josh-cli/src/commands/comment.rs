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
    let head = repo.head()?.peel_to_commit()?.id();

    let change = josh_changes::resolve_change(repo, head, &args.change)?;
    josh_changes::write_comment(repo, &change, &args.message)?;

    println!("Comment saved.");
    Ok(())
}
