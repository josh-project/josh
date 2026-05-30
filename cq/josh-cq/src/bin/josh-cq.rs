use std::io::Read;

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
}

#[derive(clap::Parser)]
struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    port: u16,
    /// WebSocket URL of the webhook relay server
    #[arg(long)]
    webhook_relay: Option<String>,
    /// Auth token for the webhook relay
    #[arg(long, env = "JOSH_CQ_WEBHOOK_TOKEN", hide_env_values = true)]
    webhook_relay_token: Option<String>,
    /// Queue tick interval in seconds (default: 600 = 10 minutes)
    #[arg(long, default_value = "600")]
    tick_interval: u64,
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

async fn run_serve(args: ServeArgs, data_dir: Option<&std::path::Path>) -> anyhow::Result<()> {
    let (repo_path, cache, _transaction) = open_repo(data_dir)?;

    let event_tx = josh_cq::server::spawn_serve_task(
        repo_path,
        cache,
        args.tick_interval,
        None,
        std::collections::HashMap::new(),
    );
    let app = josh_cq::server::make_router(event_tx);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], args.port));
    println!("Listening on {}", addr);

    let relay_token = args.webhook_relay_token.or_else(load_webhook_relay_token);

    let _webhook_client = match (args.webhook_relay, relay_token) {
        (Some(ws_url), Some(auth_token)) => {
            let config = josh_test_webhook_client::WebhookClientConfig {
                ws_url,
                auth_token,
                webhook_url: format!("http://{}/v1/webhook", addr),
            };
            println!("Forwarding webhooks from {}", config.ws_url);
            Some(josh_test_webhook_client::connect(&config).await?)
        }
        (None, None) => None,
        _ => {
            return Err(anyhow::anyhow!(
                "--webhook-relay and --webhook-relay-token must be provided together"
            ));
        }
    };

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Try to load the webhook relay token from
/// `~/.config/josh-cli/webhook-relay-token`.
fn load_webhook_relay_token() -> Option<String> {
    let path = dirs::config_dir()?
        .join("josh-cli")
        .join("webhook-relay-token");

    let mut file = std::fs::File::open(&path).ok()?;
    let mut token = String::new();
    file.read_to_string(&mut token).ok()?;
    let token = token.trim().to_string();

    if token.is_empty() { None } else { Some(token) }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let (_repo_path, _cache, transaction) = open_repo(cli.data_dir.as_deref())?;
            let msg = josh_cq::init::handle_init(&transaction)?;
            println!("{}", msg);
        }
        Commands::Serve(args) => run_serve(args, cli.data_dir.as_deref()).await?,
    }

    Ok(())
}
