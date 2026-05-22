use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::Deserialize;
use tokio::sync::mpsc;

use josh_core::cache::{CacheStack, TransactionContext};
use josh_core::filter::tree;
use josh_core::git::spawn_git_command;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_link::make_signature;

#[derive(Deserialize)]
pub struct TrackRequest {
    pub url: String,
    pub id: String,
    #[serde(default = "default_mode")]
    pub mode: String,
}

pub enum CqEvent {
    Track(TrackRequest),
    Webhook(WebhookPayload),
}

fn default_mode() -> String {
    "snapshot".to_string()
}

pub fn handle_track(
    url: &str,
    id: &str,
    mode: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<String> {
    let repo = transaction.repo();

    let refs = crate::remote::list_refs(url)?;

    spawn_git_command(repo.path(), &["fetch", url, "HEAD"], &[])?;

    let fetch_head_ref = repo
        .find_reference("FETCH_HEAD")
        .context("Failed to find FETCH_HEAD")?;
    let fetched_commit = fetch_head_ref
        .peel_to_commit()
        .context("Failed to peel FETCH_HEAD to commit")?
        .id();

    let head_ref = repo.head().context("Failed to get HEAD")?;
    let head_commit = head_ref
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let signature = make_signature(repo)?;

    let link_mode = josh_core::filter::LinkMode::parse(mode)
        .with_context(|| format!("Invalid link mode: '{}'", mode))?;

    let link_path = std::path::Path::new("remotes").join(id).join("link");
    let tree_with_link_oid = josh_link::prepare_link_add(
        transaction,
        &link_path,
        url,
        None,
        "HEAD",
        fetched_commit,
        &head_tree,
        link_mode,
    )?
    .into_tree_oid();

    let tree_with_link = repo
        .find_tree(tree_with_link_oid)
        .context("Failed to find tree with link")?;

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

    let refs_path = std::path::Path::new("remotes").join(id).join("refs.json");

    let final_tree = tree::insert(
        repo,
        &tree_with_link,
        &refs_path,
        refs_blob,
        git2::FileMode::Blob.into(),
    )
    .context("Failed to insert refs.json into tree")?;

    let final_commit = repo
        .commit(
            None,
            &signature,
            &signature,
            &format!("Track remote: {}", id),
            &final_tree,
            &[&head_commit],
        )
        .context("Failed to create final commit")?;

    repo.head()?
        .set_target(final_commit, "josh-cq track")
        .context("Failed to update HEAD")?;

    Ok(format!(
        "Tracked remote '{}' at {}\nFound {} refs",
        id,
        url,
        refs.len()
    ))
}

async fn track_handler(
    State(event_tx): State<mpsc::Sender<CqEvent>>,
    axum::Json(req): axum::Json<TrackRequest>,
) -> impl IntoResponse {
    enqueue(&event_tx, CqEvent::Track(req)).await
}

async fn webhook_handler(
    State(event_tx): State<mpsc::Sender<CqEvent>>,
    payload: WebhookPayload,
) -> impl IntoResponse {
    enqueue(&event_tx, CqEvent::Webhook(payload)).await
}

async fn enqueue(event_tx: &mpsc::Sender<CqEvent>, event: CqEvent) -> (StatusCode, &'static str) {
    match event_tx.send(event).await {
        Ok(()) => (StatusCode::ACCEPTED, "accepted"),
        Err(e) => {
            tracing::error!(error = ?e, "failed to enqueue event");
            (StatusCode::SERVICE_UNAVAILABLE, "event queue closed")
        }
    }
}

pub fn make_router(event_tx: mpsc::Sender<CqEvent>) -> axum::Router {
    axum::Router::new()
        .route("/v1/track", post(track_handler))
        .route("/v1/webhook", post(webhook_handler))
        .with_state(event_tx)
}

pub fn spawn_serve_task(repo_path: PathBuf, cache: Arc<CacheStack>) -> mpsc::Sender<CqEvent> {
    let (event_tx, mut event_rx) = mpsc::channel::<CqEvent>(100);

    tokio::task::spawn_blocking(move || {
        while let Some(event) = event_rx.blocking_recv() {
            match event {
                CqEvent::Track(req) => {
                    let transaction =
                        match TransactionContext::new(&repo_path, cache.clone()).open(None) {
                            Ok(t) => t,
                            Err(e) => {
                                eprintln!("Failed to open transaction: {e:#}");
                                continue;
                            }
                        };
                    match handle_track(&req.url, &req.id, &req.mode, &transaction) {
                        Ok(msg) => println!("{msg}"),
                        Err(e) => eprintln!("track failed: {e:#}"),
                    }
                }
                CqEvent::Webhook(payload) => {
                    println!("received webhook: {payload:?}");
                }
            }
        }
    });

    event_tx
}
