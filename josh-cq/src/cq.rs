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

#[derive(Debug, Clone)]
pub struct CandidatePr {
    pub node_id: String,
    pub number: i64,
    pub repo_url: String,
    pub head_sha: String,
    pub head_branch: String,
    pub base_sha: String,
    pub base_branch: String,
    pub title: String,
}

#[derive(Default, Clone)]
pub struct CqActorState {
    pub admission: BTreeMap<String, BTreeSet<RequiredStatusCheck>>,
    pub pr_admissions: BTreeMap<String, AdmissionState>,
    pub candidates: BTreeMap<String, CandidatePr>,
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

    pub fn upsert_candidate(&mut self, pr: CandidatePr) {
        self.candidates.insert(pr.node_id.clone(), pr);
    }

    pub fn remove_candidate(&mut self, pr_node_id: &str) {
        self.candidates.remove(pr_node_id);
        self.pr_admissions.remove(pr_node_id);
    }

    pub fn get_candidate(&self, pr_node_id: &str) -> Option<&CandidatePr> {
        self.candidates.get(pr_node_id)
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

    match payload {
        WebhookPayload::PullRequest(e) => {
            let pr = &e.pull_request;
            match &e.details {
                webhook_types::PullRequestEventDetails::Opened
                | webhook_types::PullRequestEventDetails::Synchronize { .. } => {
                    state.upsert_candidate(CandidatePr {
                        node_id: pr.node_id.clone(),
                        number: pr.number,
                        repo_url: clone_url.clone(),
                        head_sha: pr.head.sha(),
                        head_branch: pr.head.reference(),
                        base_sha: pr.base.sha(),
                        base_branch: pr.base.reference(),
                        title: pr.title.clone(),
                    });
                    state.get_or_init_pr_admission(&pr.node_id, clone_url, api);
                }
                webhook_types::PullRequestEventDetails::Closed => {
                    state.remove_candidate(&pr.node_id);
                }
                _ => {}
            }
        }

        WebhookPayload::Push(e) => {
            let pushed_ref = &e.ref_;
            for candidate in state.candidates.values_mut() {
                if candidate.repo_url == *clone_url && candidate.base_branch == *pushed_ref {
                    candidate.base_sha = e.after.clone();
                }
            }
        }

        WebhookPayload::PullRequestReview(e) => {
            let events = vec![(
                e.pull_request.node_id.clone(),
                AdmissionRelevantEvent::PullRequestReview(e),
            )];
            process_admission_events(&mut state, &events, clone_url, api);
        }

        WebhookPayload::CheckRun(e) => {
            let pr_ids = lookup_open_prs_by_sha(api, clone_url, &e.check_run.head_sha);
            let event = AdmissionRelevantEvent::CheckRun(e);
            let events: Vec<_> = pr_ids.into_iter().map(|id| (id, event)).collect();
            process_admission_events(&mut state, &events, clone_url, api);
        }

        WebhookPayload::Ping(_)
        | WebhookPayload::WorkflowJob(_)
        | WebhookPayload::WorkflowRun(_) => {}
    }

    Ok(state)
}

pub fn handle_fetch(
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    mut state: CqActorState,
) -> anyhow::Result<CqActorState> {
    let repo = transaction.repo();
    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let mut remotes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, filter) in &link_files {
        if let (Some(remote), Some(commit)) = (filter.get_meta("remote"), filter.get_meta("commit"))
        {
            remotes.push((path.clone(), remote, commit));
        }
    }

    if remotes.is_empty() {
        tracing::info!("no tracked remotes found");
        return Ok(state);
    }

    let signature = josh_link::make_signature(repo)?;
    let mut links_to_update: Vec<(PathBuf, git2::Oid)> = Vec::new();

    for (path, url, current_commit) in &remotes {
        spawn_git_command(repo.path(), &["fetch", url.as_str()], &[])
            .with_context(|| format!("Failed to fetch from {}", url))?;

        let refs = crate::remote::list_refs(url)
            .with_context(|| format!("Failed to list refs for {}", url))?;

        if let Some(head_oid) = refs.get("HEAD") {
            if head_oid.to_string() != *current_commit {
                links_to_update.push((path.clone(), *head_oid));
            }
        }
    }

    if !links_to_update.is_empty() {
        let count = links_to_update.len();
        match josh_link::update_links(repo, transaction, &head_commit, links_to_update, &signature)?
        {
            Some(result) => {
                repo.head()?
                    .set_target(result.commit_with_updates, "josh-cq fetch")
                    .context("Failed to update HEAD")?;
            }
            None => {
                tracing::debug!("link files already up to date");
            }
        }
        tracing::info!(count, "updated link file(s)");
    }

    for (_, url, _) in &remotes {
        let (owner, repo_name) = match josh_github_changes::repo::parse_owner_repo(url) {
            Ok(parts) => parts,
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "could not parse owner/repo");
                continue;
            }
        };

        let Some(api) = api else {
            tracing::warn!(url = %url, "skipping PR discovery: no API connection");
            continue;
        };

        let prs = match tokio::runtime::Handle::current()
            .block_on(api.get_open_pull_requests(&owner, &repo_name))
        {
            Ok(prs) => prs,
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "failed to fetch open PRs");
                continue;
            }
        };

        for pr in &prs {
            state.upsert_candidate(CandidatePr {
                node_id: pr.node_id.clone(),
                number: pr.number,
                repo_url: url.clone(),
                head_sha: pr.head_sha.clone(),
                head_branch: pr.head_branch.clone(),
                base_sha: pr.base_sha.clone(),
                base_branch: pr.base_branch.clone(),
                title: pr.title.clone(),
            });

            state.get_or_init_pr_admission(&pr.node_id, url, Some(api));

            match tokio::runtime::Handle::current()
                .block_on(api.get_pr_reviews(&owner, &repo_name, pr.number))
            {
                Ok(reviews) => {
                    if let Some(admission) = state.pr_admissions.get_mut(&pr.node_id) {
                        admission.apply_review_states(&reviews);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        pr = %pr.node_id,
                        error = ?e,
                        "failed to fetch PR reviews"
                    );
                }
            }
        }

        tracing::info!(
            url = %url,
            count = prs.len(),
            "discovered open PRs"
        );
    }

    Ok(state)
}

/// Select the first admissible PR from the candidate pool.
///
/// Iterates candidates in insertion order (BTreeMap), checks each one's
/// admission state, and returns the first that passes `admissible()`.
pub fn select_candidate(state: &CqActorState) -> Option<CandidatePr> {
    for (node_id, candidate) in &state.candidates {
        if let Some(admission) = state.pr_admissions.get(node_id) {
            if admission.admissible() {
                tracing::info!(
                    pr = %node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "selected admissible PR"
                );
                return Some(candidate.clone());
            }
        }
    }
    None
}

fn process_admission_events(
    state: &mut CqActorState,
    events: &[(String, AdmissionRelevantEvent<'_>)],
    clone_url: &str,
    api: Option<&GithubApiConnection>,
) {
    for (pr_node_id, evt) in events {
        let Some(admission) = state.get_or_init_pr_admission(pr_node_id, clone_url, api) else {
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
