use anyhow::{Context, anyhow};
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
    /// Manually mark a change as admissible, allowing it to participate
    /// in speculative history
    Admit(AdmitArgs),
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

#[derive(clap::Parser)]
struct AdmitArgs {
    /// Change ID to admit
    change_id: String,
}

// TODO: make it configurable/read git config
const METAREPO_MAIN_REF: &'static str = "refs/heads/master";

fn make_changes_path(remote_id: &str) -> std::path::PathBuf {
    std::path::Path::new("remotes")
        .join(remote_id)
        .join("changes.json")
}

fn list_remotes(repo: &git2::Repository) -> anyhow::Result<Vec<String>> {
    let metarepo_main = repo.find_reference(METAREPO_MAIN_REF)?.peel_to_commit()?;
    let metarepo_tree = metarepo_main.tree()?;

    let Some(remotes_entry) = metarepo_tree.get_name("remotes") else {
        return Ok(vec![]);
    };

    let remotes_tree = repo.find_tree(remotes_entry.id())?;

    remotes_tree
        .iter()
        .map(|entry| {
            entry
                .name()
                .map(|s| s.to_string())
                .context("Invalid remote entry name")
        })
        .collect()
}

fn read_changes(
    repo: &git2::Repository,
    tree: &git2::Tree,
    remote_id: &str,
) -> anyhow::Result<Option<josh_cq::change::ChangeGraph>> {
    let changes_path = make_changes_path(remote_id);

    let Ok(entry) = tree.get_path(&changes_path) else {
        return Ok(None);
    };

    let blob = repo.find_blob(entry.id())?;
    let graph = serde_json::from_slice(blob.content()).context("Failed to parse changes.json")?;

    Ok(Some(graph))
}

fn write_changes<'a>(
    repo: &'a git2::Repository,
    tree: &git2::Tree,
    remote_id: &str,
    changes: &josh_cq::change::ChangeGraph,
) -> anyhow::Result<git2::Tree<'a>> {
    let changes_json =
        serde_json::to_string_pretty(changes).context("Failed to serialize changes")?;
    let blob_oid = repo
        .blob(changes_json.as_bytes())
        .context("Failed to create changes.json blob")?;

    let changes_path = make_changes_path(remote_id);

    tree::insert(
        repo,
        tree,
        &changes_path,
        blob_oid,
        git2::FileMode::Blob.into(),
    )
    .context("Failed to insert changes.json into tree")
}

fn commit_metarepo_main(
    repo: &git2::Repository,
    tree: &git2::Tree,
    message: &str,
) -> anyhow::Result<git2::Oid> {
    let metarepo_main = repo.find_reference(METAREPO_MAIN_REF)?.peel_to_commit()?;
    let signature = make_signature(repo)?;

    let commit_oid = repo.commit(
        None,
        &signature,
        &signature,
        message,
        tree,
        &[&metarepo_main],
    )?;

    repo.head()?.set_target(commit_oid, message)?;

    Ok(commit_oid)
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

    let metarepo_tree = repo
        .find_reference(METAREPO_MAIN_REF)?
        .peel_to_commit()?
        .tree()
        .context("Failed to get main tree")?;

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

    let final_tree = write_changes(repo, &tree_with_link, &args.id, &changes)?;

    commit_metarepo_main(repo, &final_tree, &format!("Track remote: {}", args.id))?;

    println!("Tracked remote '{}' at {}", args.id, args.url);
    println!("Found {} changes", changes.graph.node_count());

    Ok(())
}

fn handle_admit(
    args: &AdmitArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let metarepo_tree = repo
        .find_reference(METAREPO_MAIN_REF)?
        .peel_to_commit()?
        .tree()
        .context("Failed to get main tree")?;

    let remotes = list_remotes(repo)?;
    let mut found = false;
    let mut current_tree_oid = metarepo_tree.id();

    for remote_id in &remotes {
        let current_tree = repo.find_tree(current_tree_oid)?;

        let Some(mut change_graph) = read_changes(repo, &current_tree, remote_id)? else {
            continue;
        };

        let Some(&node_idx) = change_graph.nodes.get(&args.change_id) else {
            continue;
        };

        change_graph.graph[node_idx].admit = true;
        found = true;

        let updated_tree = write_changes(repo, &current_tree, remote_id, &change_graph)?;
        current_tree_oid = updated_tree.id();
    }

    if !found {
        return Err(anyhow!(
            "Change '{}' not found in any tracked remote",
            args.change_id
        ));
    }

    let final_tree = repo.find_tree(current_tree_oid)?;

    commit_metarepo_main(
        repo,
        &final_tree,
        &format!("Admit change: {}", args.change_id),
    )?;

    println!("Admitted change '{}'", args.change_id);

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
        ActionCommands::Admit(ref args) => handle_admit(args, &transaction),
    }
}
