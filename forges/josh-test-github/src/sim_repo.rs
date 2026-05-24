use crate::git_server::GitServer;
use crate::test_repo::{HookType, TestRepo};
use crate::webhook_sender::send_webhook;
use anyhow::Context;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

const POST_RECEIVE_HOOK: &str = r#"#!/usr/bin/env bash
set -eu -o pipefail
shopt -s inherit_errexit

if [[ ! -v JOSH_TEST_HOOK_URL ]]; then
    echo "JOSH_TEST_HOOK_URL is not set" >&2
    exit 1
fi

function _append_to_array() {
    declare -n dest="$1"
    dest+=("$(</dev/stdin)")
}

function _make_json() {
    echo "{"
    echo '"updated_refs": ['

    declare -a refs

    while read -r old_rev new_rev ref_name; do
        local new_tree
        new_tree=$(git log --pretty='format:%T' -n1 "${new_rev}")

        _append_to_array refs <<EOF
        {
          "old_rev": "$old_rev",
          "new_rev": "$new_rev",
          "new_tree": "$new_tree",
          "ref_name": "$ref_name"
        }
EOF
    done

    (IFS=, ; echo "${refs[*]}")

    echo "]"
    echo "}"
}

function _make_request() {
    local json
    json="$(_make_json)"

    curl --request POST \
        --header "Content-Type: application/json" \
        --data "${json}" \
        "${JOSH_TEST_HOOK_URL}"
}

_make_request
"#;

pub struct SimRepo {
    inner: Arc<tokio::sync::Mutex<Inner>>,
    path: PathBuf,
    clone_url: Url,
    owner: String,
    name: String,
}

struct Inner {
    test_repo: TestRepo,
    git_server: GitServer,
    _hook_server: tokio::task::JoinHandle<()>,
    webhook_target: Option<SocketAddr>,
    pull_requests: Vec<SimPr>,
}

struct SimPr {
    head_ref: String,
    base_ref: String,
    base_oid: git2::Oid,
    state: SimPrState,
}

enum SimPrState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Deserialize)]
struct GitPushHookEvent {
    updated_refs: Vec<GitPushHookUpdatedRef>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitPushHookUpdatedRef {
    old_rev: String,
    new_rev: String,
    #[allow(dead_code)]
    new_tree: String,
    ref_name: String,
}

pub fn make_repository(clone_url: &str) -> webhook_types::Repository {
    josh_github_webhooks::test_helpers::make_repository(clone_url)
}

pub fn make_pr_node_id(owner: &str, name: &str, number: usize) -> String {
    josh_github_webhooks::test_helpers::make_pr_node_id(owner, name, number)
}

pub fn make_pr_payload(
    owner: &str,
    name: &str,
    number: usize,
    head_ref: &str,
    head_sha: &str,
    base_ref: &str,
    base_sha: &str,
) -> webhook_types::PullRequest {
    josh_github_webhooks::test_helpers::make_pr_payload(
        owner, name, number, head_ref, head_sha, base_ref, base_sha,
    )
}

impl Inner {
    fn resolve_ref(&self, ref_name: &str) -> anyhow::Result<git2::Oid> {
        let reference = self.test_repo.repo().find_reference(ref_name)?;
        let reference = reference.peel_to_commit()?;
        Ok(reference.id())
    }

    fn current_branch_ref(&self) -> String {
        self.test_repo.current_branch_ref()
    }
}

async fn run_hook_listener(
    listener: tokio::net::TcpListener,
    webhook_target: Option<SocketAddr>,
    clone_url: Url,
) {
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};

    #[derive(Clone)]
    struct HookState {
        webhook_target: Option<SocketAddr>,
        clone_url: Url,
    }

    async fn handle_push(
        State(state): State<HookState>,
        Json(push): Json<GitPushHookEvent>,
    ) -> impl IntoResponse {
        let results: Vec<anyhow::Result<()>> = push
            .updated_refs
            .into_iter()
            .map(|ref_data| {
                let event = WebhookPayload::Push(Box::new(webhook_types::PushEvent {
                    ref_: ref_data.ref_name,
                    before: ref_data.old_rev,
                    after: ref_data.new_rev,
                    repository: make_repository(state.clone_url.as_str()),
                }));

                match state.webhook_target {
                    Some(target) => send_webhook(target, &event),
                    None => Ok(()),
                }
            })
            .collect();

        for result in results {
            if let Err(e) = result {
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
        }

        StatusCode::OK.into_response()
    }

    let state = HookState {
        webhook_target,
        clone_url,
    };

    let router = Router::new()
        .route("/", post(handle_push))
        .with_state(state);

    axum::serve(listener, router).await.unwrap();
}

impl SimRepo {
    pub async fn new(
        owner: impl ToString,
        name: impl ToString,
        webhook_target: Option<SocketAddr>,
    ) -> anyhow::Result<Self> {
        let mut test_repo = TestRepo::new()?;
        let owner = owner.to_string();
        let name = name.to_string();

        // Start hook listener on a random port
        let hook_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let hook_url = format!("http://{}/", hook_listener.local_addr()?);

        // Install post-receive hook
        test_repo.install_hook(HookType::PostReceive, POST_RECEIVE_HOOK)?;

        // Start GitServer with JOSH_TEST_HOOK_URL in env
        let mut extra_env = HashMap::new();
        extra_env.insert("JOSH_TEST_HOOK_URL".to_string(), hook_url.clone());

        let git_server = GitServer::new(&test_repo.path(), extra_env).await?;
        let clone_url = git_server.url();
        let path = test_repo.path();

        let hook_server = {
            let clone_url = clone_url.clone();
            tokio::spawn(async move {
                run_hook_listener(hook_listener, webhook_target, clone_url).await;
            })
        };

        let inner = Inner {
            test_repo,
            git_server,
            _hook_server: hook_server,
            webhook_target,
            pull_requests: Vec::new(),
        };

        Ok(Self {
            inner: Arc::new(tokio::sync::Mutex::new(inner)),
            path,
            clone_url,
            owner,
            name,
        })
    }

    pub fn clone_url(&self) -> Url {
        self.clone_url.clone()
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    async fn with_inner<F, R>(&self, f: F) -> anyhow::Result<R>
    where
        F: FnOnce(&mut Inner) -> anyhow::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut inner = inner.blocking_lock();
            f(&mut inner)
        })
        .await?
    }

    /// Commit a file to the current branch. Sends a PushEvent webhook.
    pub async fn commit(
        &self,
        file_path: &str,
        content: &str,
        message: Option<&str>,
    ) -> anyhow::Result<(git2::Oid, git2::Oid)> {
        let file_path = file_path.to_owned();
        let content = content.to_owned();
        let message = message.map(|m| m.to_owned());
        let owner = self.owner.clone();
        let name = self.name.clone();

        self.with_inner(move |inner| {
            let before = inner
                .test_repo
                .current_head()
                .ok()
                .unwrap_or_else(git2::Oid::zero);

            let (after, tree) = inner
                .test_repo
                .commit(&file_path, &content, message.as_deref())?;

            let clone_url = inner.git_server.url().to_string();
            let branch_ref = inner.current_branch_ref();

            // Send PushEvent
            if let Some(target) = inner.webhook_target {
                let push = WebhookPayload::Push(Box::new(webhook_types::PushEvent {
                    ref_: branch_ref.clone(),
                    before: before.to_string(),
                    after: after.to_string(),
                    repository: make_repository(&clone_url),
                }));
                send_webhook(target, &push)?;
            }

            // If the current branch is a PR head, send synchronize event
            if let Some((pr_number, _pr)) = inner
                .pull_requests
                .iter()
                .enumerate()
                .find(|(_, pr)| pr.head_ref == branch_ref)
            {
                if let Some(target) = inner.webhook_target {
                    let pr = inner.pull_requests.get(pr_number).context("PR not found")?;
                    let sync =
                        WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
                            pull_request: make_pr_payload(
                                &owner,
                                &name,
                                pr_number,
                                &pr.head_ref,
                                &after.to_string(),
                                &pr.base_ref,
                                &pr.base_oid.to_string(),
                            ),
                            repository: make_repository(&clone_url),
                            details: webhook_types::PullRequestEventDetails::Synchronize {
                                before: before.to_string(),
                                after: after.to_string(),
                            },
                        }));
                    send_webhook(target, &sync)?;
                }
            }

            Ok((after, tree))
        })
        .await
    }

    /// Switch to a branch, creating it if needed.
    pub async fn select_create_branch(
        &self,
        branch_name: &str,
    ) -> anyhow::Result<Option<(git2::Oid, git2::Oid)>> {
        let branch_name = branch_name.to_owned();

        self.with_inner(move |inner| {
            let result = inner.test_repo.select_create_branch(&branch_name)?;

            if let Some((new_commit, tree_oid)) = result {
                if let Some(target) = inner.webhook_target {
                    let clone_url = inner.git_server.url().to_string();
                    let push = WebhookPayload::Push(Box::new(webhook_types::PushEvent {
                        ref_: inner.current_branch_ref(),
                        before: inner
                            .test_repo
                            .current_head()
                            .map(|o| o.to_string())
                            .unwrap_or_else(|_| git2::Oid::zero().to_string()),
                        after: new_commit.to_string(),
                        repository: make_repository(&clone_url),
                    }));
                    send_webhook(target, &push)?;
                }

                return Ok(Some((new_commit, tree_oid)));
            }

            Ok(None)
        })
        .await
    }

    /// Open a pull request. Sends a PullRequestEvent::Opened webhook.
    /// Returns the PR number (0-based index into the PR list).
    pub async fn open_pr(&self, head_branch: &str, base_branch: &str) -> anyhow::Result<usize> {
        let head_branch = head_branch.to_owned();
        let base_branch = base_branch.to_owned();
        let owner = self.owner.clone();
        let name = self.name.clone();

        self.with_inner(move |inner| {
            let head_ref = format!("refs/heads/{}", head_branch);
            let base_ref = format!("refs/heads/{}", base_branch);

            let head_sha = inner.resolve_ref(&head_ref)?.to_string();
            let base_oid = inner.resolve_ref(&base_ref)?;

            let number = inner.pull_requests.len();
            let clone_url = inner.git_server.url().to_string();

            if let Some(target) = inner.webhook_target {
                let event =
                    WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
                        pull_request: make_pr_payload(
                            &owner,
                            &name,
                            number,
                            &head_ref,
                            &head_sha,
                            &base_ref,
                            &base_oid.to_string(),
                        ),
                        repository: make_repository(&clone_url),
                        details: webhook_types::PullRequestEventDetails::Opened,
                    }));
                send_webhook(target, &event)?;
            }

            inner.pull_requests.push(SimPr {
                head_ref,
                base_ref,
                base_oid,
                state: SimPrState::Open,
            });

            Ok(number)
        })
        .await
    }

    /// Send a CheckRunEvent::Completed webhook for a PR's head commit.
    pub async fn send_check_run_completed(
        &self,
        pr_number: usize,
        check_name: &str,
        conclusion: webhook_types::CheckRunConclusion,
    ) -> anyhow::Result<()> {
        let check_name = check_name.to_owned();

        self.with_inner(move |inner| {
            let pr = inner
                .pull_requests
                .get(pr_number)
                .context("Invalid PR number")?;
            let head_sha = inner.resolve_ref(&pr.head_ref)?.to_string();
            let clone_url = inner.git_server.url().to_string();

            if let Some(target) = inner.webhook_target {
                let event = WebhookPayload::CheckRun(Box::new(webhook_types::CheckRunEvent {
                    check_run: webhook_types::CheckRun {
                        id: 0,
                        name: check_name,
                        head_sha,
                        status: "completed".to_string(),
                        conclusion: Some(conclusion),
                        started_at: Default::default(),
                        completed_at: None,
                    },
                    repository: make_repository(&clone_url),
                    details: webhook_types::CheckRunEventDetails::Completed,
                }));
                send_webhook(target, &event)?;
            }

            Ok(())
        })
        .await
    }

    /// Send a PullRequestReviewEvent::Submitted webhook for a PR.
    pub async fn send_review(
        &self,
        pr_number: usize,
        reviewer_login: &str,
        state: webhook_types::PullRequestReviewState,
    ) -> anyhow::Result<()> {
        let reviewer_login = reviewer_login.to_owned();
        let owner = self.owner.clone();
        let name = self.name.clone();

        self.with_inner(move |inner| {
            let pr = inner
                .pull_requests
                .get(pr_number)
                .context("Invalid PR number")?;
            let head_sha = inner.resolve_ref(&pr.head_ref)?.to_string();
            let clone_url = inner.git_server.url().to_string();

            if let Some(target) = inner.webhook_target {
                let event = WebhookPayload::PullRequestReview(Box::new(
                    webhook_types::PullRequestReviewEvent {
                        review: webhook_types::PullRequestReview {
                            id: 0,
                            user: webhook_types::User {
                                login: reviewer_login,
                            },
                            body: None,
                            commit_id: head_sha.clone(),
                            submitted_at: Default::default(),
                            state,
                        },
                        pull_request: make_pr_payload(
                            &owner,
                            &name,
                            pr_number,
                            &pr.head_ref,
                            &head_sha,
                            &pr.base_ref,
                            &pr.base_oid.to_string(),
                        ),
                        repository: make_repository(&clone_url),
                        details: webhook_types::PullRequestReviewEventDetails::Submitted,
                    },
                ));
                send_webhook(target, &event)?;
            }

            Ok(())
        })
        .await
    }

    /// Send a PullRequestEvent::Closed webhook (e.g., for merge simulation).
    pub async fn send_pr_closed(&self, pr_number: usize) -> anyhow::Result<()> {
        let owner = self.owner.clone();
        let name = self.name.clone();

        self.with_inner(move |inner| {
            let pr = inner
                .pull_requests
                .get(pr_number)
                .context("Invalid PR number")?;
            let head_sha = inner.resolve_ref(&pr.head_ref)?.to_string();
            let clone_url = inner.git_server.url().to_string();

            if let Some(target) = inner.webhook_target {
                let event =
                    WebhookPayload::PullRequest(Box::new(webhook_types::PullRequestEvent {
                        pull_request: make_pr_payload(
                            &owner,
                            &name,
                            pr_number,
                            &pr.head_ref,
                            &head_sha,
                            &pr.base_ref,
                            &pr.base_oid.to_string(),
                        ),
                        repository: make_repository(&clone_url),
                        details: webhook_types::PullRequestEventDetails::Closed,
                    }));
                send_webhook(target, &event)?;
            }

            inner.pull_requests[pr_number].state = SimPrState::Closed;
            Ok(())
        })
        .await
    }
}
