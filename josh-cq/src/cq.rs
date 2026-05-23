use std::collections::{BTreeMap, BTreeSet};
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
use josh_github_changes::admission::AdmissionState;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;
use josh_link::make_signature;

const GH_TOKEN_ENV: &str = "GH_TOKEN";

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

#[derive(Clone, Copy)]
pub enum AdmissionRelevantEvent<'a> {
    PullRequestReview(&'a webhook_types::PullRequestReviewEvent),
    CheckRun(&'a webhook_types::CheckRunEvent),
}

#[derive(Default, Clone)]
pub struct CqActorState {
    pub admission: BTreeMap<String, BTreeSet<RequiredStatusCheck>>,
    pub pr_admissions: BTreeMap<String, AdmissionState>,
}

impl CqActorState {
    pub fn get_or_fetch_admission(
        &mut self,
        clone_url: &str,
        api: Option<&GithubApiConnection>,
    ) -> Option<BTreeSet<RequiredStatusCheck>> {
        if let Some(checks) = self.admission.get(clone_url) {
            return Some(checks.clone());
        }

        let Some(api) = api else {
            tracing::warn!(
                url = %clone_url,
                "skipping admission populate: {} not set",
                GH_TOKEN_ENV
            );
            return None;
        };

        let (owner, name) = match josh_github_changes::repo::parse_owner_repo(clone_url) {
            Ok(parts) => parts,
            Err(e) => {
                tracing::warn!(url = %clone_url, error = ?e, "could not parse owner/repo");
                return None;
            }
        };

        match tokio::runtime::Handle::current().block_on(fetch_required_checks(api, &owner, &name))
        {
            Ok(checks) => {
                tracing::info!(
                    url = %clone_url,
                    count = checks.len(),
                    "populated admission entry"
                );
                self.admission.insert(clone_url.to_string(), checks.clone());
                Some(checks)
            }
            Err(e) => {
                tracing::error!(
                    url = %clone_url,
                    error = ?e,
                    "failed to fetch required checks; will retry on next webhook"
                );
                None
            }
        }
    }

    pub fn get_or_init_pr_admission(
        &mut self,
        pr_node_id: &str,
        clone_url: &str,
        api: Option<&GithubApiConnection>,
    ) -> Option<&mut AdmissionState> {
        if !self.pr_admissions.contains_key(pr_node_id) {
            let required = self.get_or_fetch_admission(clone_url, api)?;
            let maintainers = fetch_maintainers(clone_url, api);
            let state = AdmissionState {
                required_checks: required.into_iter().map(|c| (c, false)).collect(),
                maintainer_reviews: BTreeMap::new(),
                maintainers: maintainers.into_iter().collect(),
            };
            tracing::info!(
                pr = %pr_node_id,
                url = %clone_url,
                "initialized pr_admission entry"
            );
            self.pr_admissions.insert(pr_node_id.to_string(), state);
        }
        self.pr_admissions.get_mut(pr_node_id)
    }
}

fn fetch_maintainers(clone_url: &str, api: Option<&GithubApiConnection>) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let (owner, name) = match josh_github_changes::repo::parse_owner_repo(clone_url) {
        Ok(parts) => parts,
        Err(_) => return Vec::new(),
    };
    match tokio::runtime::Handle::current().block_on(api.get_maintainers(&owner, &name)) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(url = %clone_url, error = ?e, "failed to fetch maintainers");
            Vec::new()
        }
    }
}

fn lookup_open_prs_by_sha(
    api: Option<&GithubApiConnection>,
    clone_url: &str,
    sha: &str,
) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let (owner, name) = match josh_github_changes::repo::parse_owner_repo(clone_url) {
        Ok(parts) => parts,
        Err(e) => {
            tracing::warn!(url = %clone_url, error = ?e, "could not parse owner/repo");
            return Vec::new();
        }
    };
    match tokio::runtime::Handle::current()
        .block_on(api.find_open_prs_by_head_sha(&owner, &name, sha))
    {
        Ok(prs) => prs.into_iter().map(|(id, _)| id).collect(),
        Err(e) => {
            tracing::warn!(url = %clone_url, sha = %sha, error = ?e, "failed to look up PRs by SHA");
            Vec::new()
        }
    }
}

pub enum UserAction {
    Message(String),
}

fn default_mode() -> String {
    "snapshot".to_string()
}

pub fn handle_init(transaction: &josh_core::cache::Transaction) -> anyhow::Result<String> {
    let repo = transaction.repo();

    if repo.head().is_ok() {
        return Ok("Already initialized".to_string());
    }

    let head_ref = repo.find_reference("HEAD").context("Failed to find HEAD")?;
    let target = head_ref
        .symbolic_target()
        .context("HEAD is not a symbolic reference")?
        .to_string();

    let signature = make_signature(repo)?;

    let empty_tree_oid = repo
        .treebuilder(None)
        .context("Failed to create tree builder")?
        .write()
        .context("Failed to write empty tree")?;
    let empty_tree = repo
        .find_tree(empty_tree_oid)
        .context("Failed to find empty tree")?;

    let commit_oid = repo
        .commit(
            Some(&target),
            &signature,
            &signature,
            "Initialize metarepo",
            &empty_tree,
            &[],
        )
        .context("Failed to create initial commit")?;

    Ok(format!(
        "Initialized metarepo on {} at {}",
        target, commit_oid
    ))
}

pub fn handle_track(
    url: &str,
    id: &str,
    mode: &str,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<UserAction> {
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

    let action = UserAction::Message(format!(
        "Tracked remote '{}' at {}\nFound {} refs",
        id,
        url,
        refs.len()
    ));

    Ok(action)
}

fn webhook_repository(payload: &WebhookPayload) -> &webhook_types::Repository {
    match payload {
        WebhookPayload::Ping(e) => &e.repository,
        WebhookPayload::Push(e) => &e.repository,
        WebhookPayload::PullRequest(e) => &e.repository,
        WebhookPayload::WorkflowJob(e) => &e.repository,
        WebhookPayload::WorkflowRun(e) => &e.repository,
        WebhookPayload::CheckRun(e) => &e.repository,
        WebhookPayload::PullRequestReview(e) => &e.repository,
    }
}

async fn fetch_required_checks(
    api: &GithubApiConnection,
    owner: &str,
    name: &str,
) -> anyhow::Result<BTreeSet<RequiredStatusCheck>> {
    let rulesets = api.get_repository_rulesets(owner, name).await?;
    let mut checks = BTreeSet::new();
    for ruleset in rulesets {
        if !ruleset.is_active() {
            continue;
        }
        match api.get_ruleset_required_checks(&ruleset.id).await {
            Ok(rs_checks) => checks.extend(rs_checks),
            Err(e) => tracing::warn!(
                ruleset = %ruleset.id,
                error = ?e,
                "failed to fetch checks for ruleset; skipping"
            ),
        }
    }

    Ok(checks)
}

pub fn handle_webhook(
    payload: &WebhookPayload,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: CqActorState,
) -> anyhow::Result<CqActorState> {
    let repo = transaction.repo();
    let clone_url = &webhook_repository(payload).clone_url;

    let head_tree = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?
        .tree()
        .context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let tracked = link_files
        .iter()
        .any(|(_, filter)| filter.get_meta("remote").as_deref() == Some(clone_url.as_str()));

    if !tracked {
        tracing::info!(url = %clone_url, "ignoring webhook from untracked repo");
        return Ok(state);
    }

    tracing::info!(url = %clone_url, "received webhook from tracked repo");

    let mut state = state;

    let event = match payload {
        WebhookPayload::PullRequestReview(e) => AdmissionRelevantEvent::PullRequestReview(e),
        WebhookPayload::CheckRun(e) => AdmissionRelevantEvent::CheckRun(e),
        _ => return Ok(state),
    };

    let events: Vec<(String, AdmissionRelevantEvent)> = match event {
        AdmissionRelevantEvent::PullRequestReview(e) => {
            vec![(e.pull_request.node_id.clone(), event)]
        }
        AdmissionRelevantEvent::CheckRun(e) => {
            lookup_open_prs_by_sha(api, clone_url, &e.check_run.head_sha)
                .into_iter()
                .map(|id| (id, event))
                .collect()
        }
    };

    for (pr_node_id, evt) in events {
        let Some(admission) = state.get_or_init_pr_admission(&pr_node_id, clone_url, api) else {
            continue;
        };

        match evt {
            AdmissionRelevantEvent::PullRequestReview(e) => {
                admission.process_pr_review_events(std::slice::from_ref(e));
            }
            AdmissionRelevantEvent::CheckRun(e) => {
                admission.process_check_run_events(std::slice::from_ref(e));
            }
        }
    }

    Ok(state)
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

fn handle_action(action: UserAction) {
    match action {
        UserAction::Message(message) => {
            eprintln!("{}", message)
        }
    }
}

pub fn spawn_serve_task(repo_path: PathBuf, cache: Arc<CacheStack>) -> mpsc::Sender<CqEvent> {
    let (event_tx, mut event_rx) = mpsc::channel::<CqEvent>(100);

    let api: Option<Arc<GithubApiConnection>> =
        GithubApiConnection::from_environment().map(Arc::new);

    if api.is_none() {
        tracing::warn!(
            "{} not set and no stored credentials found; admission map will not be populated from GitHub",
            GH_TOKEN_ENV
        );
    }

    tokio::task::spawn_blocking(move || {
        let mut state = CqActorState::default();

        while let Some(event) = event_rx.blocking_recv() {
            let transaction = match TransactionContext::new(&repo_path, cache.clone()).open(None) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to open transaction: {e:#}");
                    continue;
                }
            };

            match event {
                CqEvent::Track(req) => {
                    match handle_track(&req.url, &req.id, &req.mode, &transaction) {
                        Ok(action) => {
                            handle_action(action);
                        }
                        Err(e) => {
                            eprintln!("track failed: {e:#}");
                        }
                    };
                }
                CqEvent::Webhook(payload) => {
                    let new_state =
                        match handle_webhook(&payload, &transaction, api.as_deref(), state.clone())
                        {
                            Ok(state) => Some(state),
                            Err(e) => {
                                eprintln!("webhook handling error: {e}");
                                None
                            }
                        };

                    if let Some(new_state) = new_state {
                        state = new_state;
                    }
                }
            }
        }
    });

    event_tx
}
