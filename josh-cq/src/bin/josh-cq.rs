use anyhow::Context;
use clap::Parser;

use josh_core::filter::tree;
use josh_core::git::normalize_repo_path;
use josh_link::make_signature;

use josh_cq::vendor::{Vendor, make_vendor};

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

// TODO: make it configurable/read git config
const METAREPO_MAIN_REF: &'static str = "refs/heads/master";

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

    let metarepo_main = repo.find_reference(METAREPO_MAIN_REF)?.peel_to_commit()?;
    let metarepo_tree = metarepo_main.tree().context("Failed to get main tree")?;

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

    let vendor = make_vendor(
        Vendor::Generic,
        repo.path(),
        &refs,
        (head_target.clone(), *resolved_head),
    )?;
    let changes = vendor.list_changes()?;

    // Create changes.json blob
    let changes_blob = {
        let changes_json =
            serde_json::to_string_pretty(&changes).context("Failed to serialize refs to JSON")?;

        repo.blob(changes_json.as_bytes())
            .context("Failed to create changes.json blob")?
    };

    // Insert changes.json into the tree
    let changes_path = std::path::Path::new("remotes")
        .join(&args.id)
        .join("changes.json");

    let final_tree = tree::insert(
        repo,
        &tree_with_link,
        &changes_path,
        changes_blob,
        git2::FileMode::Blob.into(),
    )
    .context("Failed to insert refs.json into tree")?;

    // Create final commit with both files
    let final_commit = repo
        .commit(
            None,
            &signature,
            &signature,
            &format!("Track remote: {}", args.id),
            &final_tree,
            &[&metarepo_main],
        )
        .context("Failed to create final commit")?;

    // Update HEAD to point to the new commit
    repo.head()?
        .set_target(final_commit, "josh-cq track")
        .context("Failed to update HEAD")?;

    println!("Tracked remote '{}' at {}", args.id, args.url);
    println!("Found {} changes", changes.graph.node_count());

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

    josh_core::cache::sled_load(&repo_path.join(".git")).context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default()),
    );

    let transaction = josh_core::cache::TransactionContext::new(&repo_path, cache.clone())
        .open(None)
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
