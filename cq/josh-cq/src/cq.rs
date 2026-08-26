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
    let refs = crate::remote::list_refs(url)?;

    transaction.spawn_git(&["fetch", url, "HEAD"], &[])?;

    // This fetch requests exactly one remote revision, so FETCH_HEAD has one merge candidate.
    let fetched_commit = transaction
        .rev_parse("FETCH_HEAD^{commit}")?
        .context("Failed to peel FETCH_HEAD to commit")?;

    let head = transaction.head().context("Failed to get HEAD")?;

    let signature = make_signature(transaction)?;

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
        josh_core::objects::CommitData::read(transaction.odb(), head.commit)?.tree_id()?,
        link_mode,
    )?
    .into_tree_oid();

    let odb = transaction.odb();
    let refs_blob = {
        let refs_map: BTreeMap<String, String> = refs
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();

        let refs_json =
            serde_json::to_string_pretty(&refs_map).context("Failed to serialize refs to JSON")?;

        josh_core::objects::write_blob(odb, refs_json.as_bytes())
            .context("Failed to create refs.json blob")?
    };

    let refs_path = std::path::Path::new("remotes").join(id).join("refs.json");

    let final_tree = tree::insert_oid(odb, tree_with_link_oid, &refs_path, refs_blob, 0o100644)
        .context("Failed to insert refs.json into tree")?;

    let final_commit = josh_core::objects::write_commit(
        odb,
        final_tree,
        &[head.commit],
        &signature,
        &signature,
        &format!("Track remote: {}", id),
    )
    .context("Failed to create final commit")?;

    transaction
        .update_ref(
            &head.reference,
            josh_core::cache::Expected::At(head.target),
            final_commit,
            "josh-cq track",
        )
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
                        match TransactionContext::new(&repo_path, cache.clone()).open() {
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
