use anyhow::{Context, anyhow};

#[derive(Debug, clap::Parser)]
pub struct LinkArgs {
    /// Link subcommand
    #[command(subcommand)]
    pub command: LinkCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum LinkCommand {
    /// Add a link to a view of a remote repository
    Add(LinkAddArgs),
    /// Remove a link
    Rm(LinkRmArgs),
    /// List links
    List,
}

#[derive(Debug, clap::Parser)]
pub struct LinkAddArgs {
    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Filter defining the view (e.g. :/subfolder)
    #[arg()]
    pub filter: String,

    /// Link id (defaults to the repository name derived from the URL)
    #[arg(long = "id")]
    pub id: Option<String>,
}

#[derive(Debug, clap::Parser)]
pub struct LinkRmArgs {
    /// Link id
    #[arg()]
    pub id: String,
}

pub fn handle_link(
    args: &LinkArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_core::filter::check_experimental_features_enabled("josh link")?;
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args, transaction),
        LinkCommand::Rm(rm_args) => handle_link_rm(rm_args, transaction),
        LinkCommand::List => handle_link_list(transaction),
    }
}

fn handle_link_add(
    args: &LinkAddArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let id = match &args.id {
        Some(id) => id.clone(),
        None => josh_view::derive_id_from_url(&args.url).ok_or_else(|| {
            anyhow!(
                "Could not derive a link id from URL '{}'; pass --id",
                args.url
            )
        })?,
    };
    josh_view::validate_link_id(&id)?;

    if transaction
        .resolve_ref(&josh_view::link_ref_name(&id))?
        .is_some()
    {
        return Err(anyhow!("Link '{id}' already exists"));
    }

    let filter = josh_core::filter::parse(&args.filter)
        .with_context(|| format!("Failed to parse filter '{}'", args.filter))?;

    let tracked_ref =
        crate::remote_ops::get_head_branch(&args.url, &transaction.path().to_path_buf(), &id)
            .with_context(|| format!("Failed to determine default branch of '{}'", args.url))?;

    let view = josh_view::View {
        filter,
        link: Some(josh_view::Link {
            url: args.url.clone(),
            tracked_ref: tracked_ref.clone(),
        }),
    };

    josh_view::write_view(transaction, &id, &view, &format!("josh link add {id}"))?;

    println!(
        "Added link '{id}': {} {} {}",
        args.url,
        tracked_ref,
        josh_core::filter::spec(filter)
    );
    Ok(())
}

fn handle_link_rm(
    args: &LinkRmArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_view::delete_view(transaction, &args.id)?;
    println!("Removed link '{}'", args.id);
    Ok(())
}

fn handle_link_list(transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    for (id, view) in josh_view::list_views(transaction)? {
        match &view.link {
            Some(link) => println!(
                "{}\t{}\t{}\t{}",
                id,
                link.url,
                link.tracked_ref,
                josh_core::filter::spec(view.filter)
            ),
            None => println!("{}\t{}", id, josh_core::filter::spec(view.filter)),
        }
    }
    Ok(())
}
