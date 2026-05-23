# Step 13: Add `SimRepo` and webhook event generation

## Why

`TestRepo` and `GitServer` provide a bare git repo served over HTTP, but the
merge queue needs webhook events (PR opened, checks completed, reviews submitted)
and the ability to simulate GitHub PR lifecycles. `SimRepo` wraps `TestRepo` +
`GitServer` and adds:

- Post-receive hook that forwards push events as webhooks
- PR management (open, close) that sends `PullRequestEvent` webhooks
- Check run and review event generation
- Webhook forwarding to the CQ server's `/v1/webhook` endpoint

## What to change

### File: `forges/josh-test-github/src/lib.rs`

Add modules:
```rust
pub mod git_server;
pub mod sim_repo;
pub mod test_repo;
pub mod webhook_sender;
```

### File: `forges/josh-test-github/src/webhook_sender.rs` (new)

Low-level helper: serializes a `WebhookPayload` and POSTs it to a target.

```rust
use josh_github_webhooks::webhook_server::WebhookPayload;
use std::net::SocketAddr;

/// POST a webhook payload to the CQ server.
pub fn send_webhook(
    target: SocketAddr,
    payload: &WebhookPayload,
) -> anyhow::Result<()> {
    let tagged = serde_json::to_value(payload)?;
    let obj = tagged.as_object().context("...")?;
    let event_type = obj.get("type").context("...")?.as_str().context("...")?;
    let data = obj.get("data").context("...")?;

    let url = format!("http://{}/v1/webhook", target);
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("X-GitHub-Event", event_type)
        .json(data)
        .send()?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("webhook POST failed: {}", response.status()));
    }
    Ok(())
}
```

The serialization mirrors `WebhookPayload`'s `serde` tag—matches on the `"type"`
field (`"push"`, `"pull_request"`, `"check_run"`, `"pull_request_review"`) and
extracts the inner `"data"` payload.

### File: `forges/josh-test-github/src/sim_repo.rs` (new)

High-level simulated GitHub repository.

```rust
use crate::git_server::GitServer;
use crate::test_repo::{HookType, TestRepo, INITIAL_BRANCH_REF};
use crate::webhook_sender::send_webhook;
use josh_github_webhooks::webhook_server::WebhookPayload;
use josh_github_webhooks::webhook_types;
use std::net::SocketAddr;
use std::path::PathBuf;
use url::Url;

pub struct SimRepo {
    test_repo: TestRepo,
    git_server: GitServer,
    webhook_target: Option<SocketAddr>,
    owner: String,
    name: String,
    pull_requests: Vec<SimPr>,
}

struct SimPr {
    head_ref: String,        // "refs/heads/feature-branch"
    base_ref: String,        // "refs/heads/main"
    base_oid: git2::Oid,
    state: SimPrState,
}

enum SimPrState {
    Open,
    Closed,
}
```

#### Constructor

```rust
impl SimRepo {
    pub async fn new(
        owner: impl ToString,
        name: impl ToString,
        webhook_target: Option<SocketAddr>,
    ) -> anyhow::Result<Self>
```

Creates a `TestRepo`, installs the post-receive hook (see below), starts the
`GitServer`, and starts an internal hook listener server. The hook listener
receives POSTs from the post-receive hook and forwards them as `PushEvent`
webhooks to `webhook_target`.

#### Post-receive hook architecture

The hook script (embedded as a string, adapted from metahead's
`post_receive_hook.sh`) reads `old_rev new_rev ref_name` from stdin, determines
the new tree OID via `git log -1 --format='%T'`, and POSTs a JSON array to an
internal hook listener URL. The hook listener (a small axum server inside the
`SimRepo`) converts each updated ref into a `WebhookPayload::Push` and calls
`send_webhook()`.

The hook listener URL is passed to the hook script via the `JOSH_TEST_HOOK_URL`
environment variable, which is injected into the `GitServer`'s extra env.

#### Public methods

```rust
impl SimRepo {
    pub fn clone_url(&self) -> Url { self.git_server.url() }
    pub fn path(&self) -> PathBuf { self.test_repo.path() }

    /// Commit a file to the current branch. Sends a PushEvent webhook.
    pub async fn commit(
        &self,
        file_path: &str,
        content: &str,
        message: Option<&str>,
    ) -> anyhow::Result<(git2::Oid, git2::Oid)>

    /// Switch to a branch, creating it if needed.
    pub async fn select_create_branch(&self, branch_name: &str) -> anyhow::Result<()>

    /// Open a pull request. Sends a PullRequestEvent::Opened webhook.
    /// Returns the PR number (0-based index into the PR list).
    pub async fn open_pr(
        &self,
        head_branch: &str,   // e.g., "feature-branch" (without refs/heads/)
        base_branch: &str,   // e.g., "main"
    ) -> anyhow::Result<usize>

    /// Send a CheckRunEvent::Completed webhook for a PR's head commit.
    pub async fn send_check_run_completed(
        &self,
        pr_number: usize,
        check_name: &str,
        conclusion: webhook_types::CheckRunConclusion,
    ) -> anyhow::Result<()>

    /// Send a PullRequestReviewEvent::Submitted webhook for a PR.
    pub async fn send_review(
        &self,
        pr_number: usize,
        reviewer_login: &str,
        state: webhook_types::PullRequestReviewState,
    ) -> anyhow::Result<()>

    /// Send a PullRequestEvent::Closed webhook (e.g., for merge simulation).
    pub async fn send_pr_closed(&self, pr_number: usize) -> anyhow::Result<()>
}
```

#### Thread safety

`SimRepo` uses `Arc<tokio::sync::Mutex<Inner>>` internally so that `commit`,
`open_pr`, etc. serialize access to the git repo (git2 repos are not `Send +
Sync`). All mutating operations go through a `spawn_blocking` → `blocking_lock`
pattern.

### Acceptance

- `cargo build -p josh-test-github` succeeds
- `cargo fmt` passes
