use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::Deserialize;

use josh_core::cache::{CacheStack, TransactionContext};
use josh_core::filter::tree;
use josh_core::git::spawn_git_command;
use josh_link::make_signature;

#[derive(Clone)]
pub struct AppState {
    pub repo_path: PathBuf,
    pub cache: Arc<CacheStack>,
}

#[derive(Deserialize)]
pub struct TrackRequest {
    pub url: String,
    pub id: String,
    #[serde(default = "default_mode")]
    pub mode: String,
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
    State(state): State<AppState>,
    axum::Json(req): axum::Json<TrackRequest>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        let transaction = TransactionContext::new(&state.repo_path, state.cache.clone())
            .open(None)
            .context("Failed TransactionContext::open")?;

        handle_track(&req.url, &req.id, &req.mode, &transaction)
    })
    .await;

    match result {
        Ok(Ok(msg)) => (StatusCode::OK, msg),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {e:#}")),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Task failed: {e}"),
        ),
    }
}

pub fn make_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/v1/track", post(track_handler))
        .with_state(state)
}
