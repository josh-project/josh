/// Arguments for `josh changes comment`.
#[derive(Debug, clap::Parser)]
pub struct CommentArgs {
    /// Change to comment on (Change-Id, ref, or SHA).
    #[arg()]
    pub change: String,

    /// Comment message.
    #[arg(short = 'm', long = "message")]
    pub message: String,

    /// File path the comment relates to.
    #[arg(long = "file")]
    pub file: Option<String>,

    /// Location as path:line (shortcut for --location-path PATH --location-start-line N ...).
    #[arg(long = "location")]
    pub location: Option<String>,

    /// Location path (file or directory).
    #[arg(long = "location-path")]
    pub location_path: Option<String>,

    /// Location start line (1-based).
    #[arg(long = "location-start-line")]
    pub location_start_line: Option<u32>,

    /// Location end line (1-based).
    #[arg(long = "location-end-line")]
    pub location_end_line: Option<u32>,

    /// Location start column (1-based).
    #[arg(long = "location-start-col")]
    pub location_start_col: Option<u32>,

    /// Location end column (1-based).
    #[arg(long = "location-end-col")]
    pub location_end_col: Option<u32>,

    /// Hash of a previous comment to reply to.
    #[arg(long = "reply-to")]
    pub reply_to: Option<String>,

    /// Hash of a previous comment to update/replace.
    #[arg(long = "update-of")]
    pub update_of: Option<String>,

    /// Target branch for the comment (defaults to HEAD's branch).
    #[arg(short = 'b', long = "branch")]
    pub branch: Option<String>,

    /// Queue the comment in the outbox of the named remote so the next
    /// `sync --push` will post it. Without this flag, the comment stays
    /// private to the local changes ref.
    #[arg(long = "remote")]
    pub remote: Option<String>,
}

pub fn handle_comment(
    args: &CommentArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let head = repo.head()?.peel_to_commit()?.id();

    let (short_file, short_location) = args
        .location
        .as_ref()
        .and_then(|s| {
            let (path, line) = s.rsplit_once(':')?;
            let line: u32 = line.parse().ok()?;
            Some((
                Some(path.to_string()),
                Some(josh_changes::Location {
                    start_line: line,
                    end_line: line,
                    start_col: 1,
                    end_col: u32::MAX,
                }),
            ))
        })
        .unwrap_or((None, None));

    let (file, location) = if args.location_path.is_some()
        || args.location_start_line.is_some()
        || args.location_end_line.is_some()
        || args.location_start_col.is_some()
        || args.location_end_col.is_some()
    {
        let path = args.location_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--location-path is required when using long-form location flags")
        })?;
        let start_line = args.location_start_line.ok_or_else(|| {
            anyhow::anyhow!("--location-start-line is required when using long-form location flags")
        })?;
        let end_line = args.location_end_line.ok_or_else(|| {
            anyhow::anyhow!("--location-end-line is required when using long-form location flags")
        })?;
        let start_col = args.location_start_col.ok_or_else(|| {
            anyhow::anyhow!("--location-start-col is required when using long-form location flags")
        })?;
        let end_col = args.location_end_col.ok_or_else(|| {
            anyhow::anyhow!("--location-end-col is required when using long-form location flags")
        })?;
        (
            Some(path.to_string()),
            Some(josh_changes::Location {
                start_line,
                end_line,
                start_col,
                end_col,
            }),
        )
    } else {
        (short_file, short_location)
    };

    let change = josh_changes::resolve_change(repo, head, &args.change)?;
    let meta = josh_changes::CommentMeta {
        message: args.message.clone(),
        file: file.or(args.file.clone()),
        location,
        reply_to: args.reply_to.clone(),
        update_of: args.update_of.clone(),
    };
    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => josh_changes::head_branch(repo)?,
    };

    match &args.remote {
        Some(remote) => {
            let scope = josh_changes::ChangesRef::Remote {
                remote: remote.clone(),
                branch,
            };
            josh_changes::write_outbox_comment(repo, &change, &meta, None, None, &scope)?;
            println!("Comment queued in outbox for remote '{}'.", remote);
        }
        None => {
            let scope = josh_changes::ChangesRef::Local { branch };
            josh_changes::write_comment(repo, &change, &meta, None, None, &scope)?;
            println!("Comment saved (private to local ref).");
        }
    }
    Ok(())
}
