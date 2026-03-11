use anyhow::Context;
use clap::Parser;

use josh_core::git::normalize_repo_path;

#[derive(Parser)]
#[command(about = "Josh Commit Queue")]
struct Cli {
    /// Path to the data directory (git repository). Defaults to current directory.
    #[arg(long, global = true)]
    data_dir: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Initialize metarepo
    Init,
    /// Start HTTP server
    Serve(ServeArgs),
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
    /// Link mode: embedded, snapshot, or pointer (defaults to snapshot)
    #[arg(long = "mode", default_value = "snapshot")]
    mode: String,
}

#[derive(clap::Parser)]
struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    port: u16,
}

fn open_repo(
    data_dir: Option<&std::path::Path>,
) -> anyhow::Result<(
    std::path::PathBuf,
    std::sync::Arc<josh_core::cache::CacheStack>,
    josh_core::cache::Transaction,
)> {
    let repo = match data_dir {
        Some(dir) => git2::Repository::open(dir).context("Failed to open git repository")?,
        None => git2::Repository::open_from_env().context("Not in a git repository")?,
    };
    let repo_path = normalize_repo_path(repo.path());

    josh_core::cache::sled_load(&repo_path.join(".git")).context("Failed to load sled cache")?;

    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default()),
    );

    let transaction = josh_core::cache::TransactionContext::new(&repo_path, cache.clone())
        .open(None)
        .context("Failed TransactionContext::open")?;

    Ok((repo_path, cache, transaction))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            // TODO
            return Ok(());
        }
        Commands::Serve(args) => {
            let (repo_path, cache, _transaction) = open_repo(cli.data_dir.as_deref())?;

            let state = josh_cq::cq::AppState { repo_path, cache };
            let app = josh_cq::cq::make_router(state);

            let addr = std::net::SocketAddr::from(([0, 0, 0, 0], args.port));
            println!("Listening on {}", addr);

            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
        }
        Commands::Action(action) => {
            let (_repo_path, _cache, transaction) = open_repo(cli.data_dir.as_deref())?;

            match action {
                ActionCommands::Track(ref args) => {
                    let msg =
                        josh_cq::cq::handle_track(&args.url, &args.id, &args.mode, &transaction)?;
                    println!("{}", msg);
                }
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
    }

    Ok(())
}
