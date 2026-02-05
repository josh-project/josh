use anyhow::Context;
use clap::Parser;

use josh_core::filter::tree;
use josh_link::{from_josh_err, make_signature, normalize_repo_path};

use std::collections::BTreeMap;

#[derive(Parser)]
#[command(about = "Josh Commit Queue")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Initialize metarepo
    Init,
    #[command(flatten)]
    Action(ActionCommands),
}

#[derive(clap::Subcommand)]
enum ActionCommands {
    /// Track a remote repository
    Track(TrackArgs),
    /// Fetch remotes, collect and record state of conditions
    Fetch,
    /// Single step through the queue, updating the state
    Step,
    /// Push updated metarepo state to remotes
    Push,
}

#[derive(clap::Parser)]
struct TrackArgs {
    /// URL of the remote to track
    url: String,
    /// ID for this remote
    id: String,
    /// Vendor type for this remote
    #[arg(long, default_value = "generic")]
    vendor: josh_cq::vendor::Vendor,
}

fn handle_track(
    args: &TrackArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    // Fetch everything from the remote
    let refs = josh_cq::remote::fetch(&repo, &args.url)?;
    let head_target = josh_cq::remote::resolve_head_symref(&args.url)?;

    let resolved_head = refs.get(&head_target).with_context(|| {
        format!(
            "Remote advertized non-existing HEAD symref target {}",
            head_target
        )
    })?;

    // Get the metarepo's current HEAD commit and tree
    let metarepo_head = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let metarepo_tree = metarepo_head.tree().context("Failed to get HEAD tree")?;

    let signature = make_signature(repo)?;

    let link_path = std::path::Path::new("remotes").join(&args.id).join("link");
    let tree_with_link_oid = josh_link::prepare_link_add(
        &transaction,
        &link_path,
        &args.url,
        None,   // filter (default :/)
        "HEAD", // target
        *resolved_head,
        &metarepo_tree,
    )?
    .into_tree_oid();

    let tree_with_link = repo
        .find_tree(tree_with_link_oid)
        .context("Failed to find tree with link")?;

    // Create refs.json blob
    let refs_blob = {
        let refs_map: BTreeMap<String, String> = refs
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();

        let refs_json =
            serde_json::to_string_pretty(&refs_map).context("Failed to serialize refs to JSON")?;

        repo.blob(refs_json.as_bytes())
            .context("Failed to create refs.json blob")?
    };

    // Insert refs.json into the tree
    let refs_path = std::path::Path::new("remotes")
        .join(&args.id)
        .join("refs.json");

    let final_tree = tree::insert(
        repo,
        &tree_with_link,
        &refs_path,
        refs_blob,
        git2::FileMode::Blob.into(),
    )
    .map_err(from_josh_err)
    .context("Failed to insert refs.json into tree")?;

    // Create final commit with both files
    let final_commit = repo
        .commit(
            None,
            &signature,
            &signature,
            &format!("Track remote: {}", args.id),
            &final_tree,
            &[&metarepo_head],
        )
        .context("Failed to create final commit")?;

    // Update HEAD to point to the new commit
    repo.head()?
        .set_target(final_commit, "josh-cq track")
        .context("Failed to update HEAD")?;

    println!("Tracked remote '{}' at {}", args.id, args.url);
    println!("Found {} refs", refs.len());

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let action = match cli.command {
        Commands::Init => {
            // TODO
            return Ok(());
        }
        Commands::Action(action) => action,
    };

    let repo = git2::Repository::open_from_env().context("Not in a git repository")?;
    let repo_path = normalize_repo_path(repo.path());

    josh_core::cache::sled_load(&repo_path.join(".git"))
        .map_err(from_josh_err)
        .context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default()),
    );

    let transaction = josh_core::cache::TransactionContext::new(&repo_path, cache.clone())
        .open(None)
        .map_err(from_josh_err)
        .context("Failed TransactionContext::open")?;

    match action {
        ActionCommands::Track(ref args) => handle_track(args, &transaction),
        ActionCommands::Fetch => {
            todo!()
        }
        ActionCommands::Step => {
            todo!()
        }
        ActionCommands::Push => {
            todo!()
        }
    }
}
