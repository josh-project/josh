* Before creating commit, always run `cargo fmt`
* When possible, keep PRs to one commit only; amend existing commit when making changes to PRs, and force push

## Project overview

Josh is a git monorepo proxy. Repos are tracked into a metarepo via `.link.josh` files that map filtered subdirectories to remotes. The proxy serves filtered views on-the-fly.

Key crates:

| Crate | Role |
|---|---|
| `josh-core` | Cache (`CacheStack`, `Transaction`), filter engine, link file discovery |
| `josh-link` | `.link.josh` file creation, link ref collection, `make_signature` |
| `josh-proxy` | HTTP proxy serving filtered views, auth middleware (Basic auth → git credential helper) |
| `josh-cli` | User-facing CLI (`josh auth login`, `josh filter`, push/PR creation) |
| `josh-cq` | **Merge queue** library + CLI binary (in `cq/josh-cq`) |
| `josh-cq-tests` | Merge queue integration tests (in `cq/josh-cq-tests`) |
| `josh-github-graphql` | GitHub GraphQL client (`GithubApiConnection`, PR/collaborator/ruleset queries) |
| `josh-github-auth` | GitHub device-flow OAuth, `GithubAuthMiddleware`, token refresh |
| `josh-github-keyring` | Credential storage (macOS Keychain or `~/.config/josh-cli/credentials.json`) |
| `josh-github-changes` | `AdmissionState` (check runs + maintainer reviews → admissible), PR creation/update, repo URL parsing |
| `josh-github-webhooks` | Webhook type definitions, payload deserialization, HMAC signature verification |
| `josh-test-webhook-service` | Webhook relay server: receives GitHub webhooks, broadcasts to WS clients |
| `josh-test-webhook-client` | WS client connecting to relay, forwards webhooks to CQ's `/v1/webhook` |
| `josh-cq-test-components` | Clean test infrastructure: `TestRepo` actor with serialized git-http-backend (in `cq/josh-cq-test-components`) |
| `josh-github-sim` | Simulated GitHub environment: Git HTTP + GraphQL via Juniper schema (in `forges/josh-github-sim`) |

## Merge queue (`cq/`)

Both the merge queue library/binary and its integration tests live under `cq/`:

```
cq/
├── josh-cq/          # library + binary
│   ├── src/
│   │   ├── bin/josh-cq.rs   # CLI entry point
│   │   ├── lib.rs           # module declarations
│   │   ├── types.rs         # CqEvent, TrackRequest
│   │   ├── models.rs        # CandidatePr, CqActorState (pure data + sync accessors)
│   │   ├── server.rs        # HTTP router, make_router, bind_router, spawn_serve_task
│   │   ├── api.rs           # GitHub API wrappers (maintainers, checks, PRs by SHA)
│   │   ├── git.rs           # GitActor, GitActorMessage, spawn_git_actor
│   │   ├── layout.rs        # Metarepo layout helpers (remote discovery, filter specs)
│   │   ├── step.rs          # handle_step, select_candidate, run_queue_cycle
│   │   ├── admission.rs     # admission-state construction + sync_required_checks
│   │   ├── refresh_remotes.rs  # refresh_remotes (fetch + discover open PRs + reconcile)
│   │   ├── webhook.rs       # handle_webhook
│   │   ├── track.rs         # handle_track
│   │   ├── init.rs          # handle_init
│   │   ├── remote.rs        # list_refs (git ls-remote)
│   │   └── util.rs          # make_signature
│   └── Cargo.toml
│
└── josh-cq-tests/   # integration tests
    ├── src/lib.rs
    ├── src/test_helpers.rs
    ├── tests/merge_queue_tests.rs
    └── Cargo.toml
```

### Subcommands

The CQ binary (`josh-cq`) supports:
- **`init`** — creates an empty metarepo commit on HEAD
- **`serve`** — starts HTTP server receiving webhooks (`/v1/webhook`) and track requests (`/v1/track`); tracking is done via the HTTP API, not a separate subcommand

### Architecture

The `serve` event loop uses an **actor model**. All input — webhook events, track
requests, and periodic polling ticks — is sent through a single `mpsc` channel.
A single async actor task (`tokio::spawn`) processes events serially with
`event_rx.recv().await`, mutating `CqActorState` without locks. No `state.clone()`
— handler functions take `&mut CqActorState`.

All git work (both git2 and `git` subprocesses) goes through a separate **git
actor** (`GitActor`) that owns the metarepo path and cache. The git actor
serializes git2 operations and `git` subprocess calls on a single blocking
thread, ensuring on-disk object writes are always visible to subsequent git2
reads with no cross-task races. Git subprocesses (`fetch`, `push`, `merge-tree`,
`commit-tree`) are run through a `CommandStack` that attaches the GitHub auth
token.

The queue cycle: **Refresh remotes** → **Evaluate** → **Step** (merge) → repeat while
admissible PRs remain.

```
Webhook → HTTP handler → CqEvent::Webhook ─┐
Track API → HTTP handler → CqEvent::Track  ─┤→ mpsc channel → actor loop
Timer (10 min) → CqEvent::Tick            ─┘
                                              │
                                    ┌─────────┘
                                    ▼
                              Process event
                              Update state
                                    │
                                    ▼
                        refresh_remotes (on Tick only)
                          Fetch all tracked remotes
                          Discover open PRs via GraphQL
                          Reconcile required checks
                                    │
                                    ▼
                        run_queue_cycle (Tick + Webhook)
                         Evaluate → Step → loop
```

- **Tick** (every 10 min) triggers `refresh_remotes` (fetches all tracked remotes,
  discovers open PRs from GitHub GraphQL, reconciles required checks against
  rulesets) then `run_queue_cycle`. Catches PRs missed by webhook delivery failures.
- **Webhooks** update admission state and candidate list immediately, then fall
  through to `run_queue_cycle`. No fetch on webhook — only admission updates.
- **Track** sends `git fetch` + `GitActorMessage::Track` to import the remote's
  FETCH_HEAD into the metarepo under `remotes/<id>/contents`. Does not trigger
  the queue cycle.
- **Merge** happens via `git merge-tree --write-tree` + `git commit-tree`, then
  force-pushes to the remote's base branch, unapplies the merge back onto the
  metarepo (keeping it a faithful pre-image of every tracked remote), and closes
  the PR on GitHub with a "Merged by Josh merge queue" comment.

### Actor loop (server.rs)

```rust
// spawn_serve_task creates the event channel, spawns the tick timer, and
// spawns the main actor loop. Returns the event sender for HTTP handlers.
pub fn spawn_serve_task(
    tick_interval_secs: u64,
    git: Arc<GitActor>,
    middleware: Arc<GithubAuthMiddleware>,
    api_url: Option<Url>,
    url_owner_map: HashMap<String, (String, String)>,
) -> mpsc::Sender<CqEvent>;

// process_event is a free function that matches on the event and runs the
// relevant handler. It returns an EventOutcome: whether a queue cycle is
// warranted (true for Tick/Webhook, false for Track) plus any completion
// signal to fire after the cycle.
async fn process_event(
    event: CqEvent,
    git: &GitActor,
    api: &GithubApiConnection,
    state: &mut CqActorState,
) -> EventOutcome;

// Actor loop (inside spawn_serve_task):
tokio::spawn(async move {
    let mut state = CqActorState { url_owner_map, ..Default::default() };
    while let Some(event) = event_rx.recv().await {
        let outcome = process_event(event, &git, &api, &mut state).await;

        if outcome.run_queue_cycle {
            run_queue_cycle(&mut state, &git, &api).await;
        }
        if let Some(tx) = outcome.done {
            let _ = tx.send(());
        }
    }
});
```

### State model

`CqActorState` (in `models.rs`):
- `admission`: `BTreeMap<String, BTreeSet<RequiredStatusCheck>>` — per-repo required checks
- `pr_admissions`: `BTreeMap<String, AdmissionState>` — per-PR review/check state
- `candidates`: `BTreeMap<String, CandidatePr>` — open PRs indexed by node_id
- `url_owner_map`: `HashMap<String, (String, String)>` — non-GitHub URL → (owner, name)
- `closed_prs`: `HashSet<String>` — PRs closed via webhook, excluded from re-discovery

`CandidatePr` (in `models.rs`):
- `node_id`, `number`, `repo_url`, `head_sha`, `base_sha`, `base_branch`, `title`
- `from_open_pr(repo_url, pr)` — build from a GraphQL `OpenPr` discovered during refresh
- `from_webhook_pr(repo_url, pr)` — build from a webhook `PullRequest` payload

Methods on `CqActorState` (pure, sync; in `models.rs`):
- `resolve_owner_repo(&self, url) -> Option<(String, String)>` — try `url_owner_map` first, then `parse_owner_repo`, logging on failure
- `upsert_candidate`, `remove_candidate`, `get_candidate`
- `remove_candidate` also removes the PR from `pr_admissions` and `closed_prs`

Admission-state construction lives in `admission.rs` as free functions taking
`&mut CqActorState` (they call the GitHub API, which `models.rs` no longer does):
- `get_or_fetch_admission(state, clone_url, api) -> Option<BTreeSet<RequiredStatusCheck>>` — lazy-populate required checks
- `get_or_init_pr_admission(state, pr_node_id, clone_url, api) -> Option<&mut AdmissionState>` — init `AdmissionState` for a PR
- `sync_required_checks(admission, required)` — reconcile checks with current rulesets, preserving known results

Helper in `api.rs`:
- `fetch_maintainers(clone_url, api, state) -> Vec<String>` — resolve maintainers for a repo
- `fetch_required_checks(api, owner, name) -> Result<BTreeSet<RequiredStatusCheck>>` — aggregate required checks from active rulesets
- `lookup_open_prs_by_sha(api, clone_url, sha, state) -> Vec<String>` — find open PRs by head SHA (used for check-run webhooks)

`AdmissionState` (in `josh-github-changes`):
- `required_checks`: `BTreeMap<RequiredStatusCheck, bool>` — check name → passed
- `maintainer_reviews`: `BTreeMap<String, PullRequestReviewState>` — login → review state
- `maintainers`: `HashSet<String>` — users with write access
- `admissible()`: ≥1 maintainer approved, no changes requested, all required checks passed

### Git actor and blocking work

All git work goes through `GitActor` (`git.rs`), which owns the metarepo path,
cache, and `CommandStack`. Callers send `GitActorMessage` variants through
`git.request(|reply| ...)`, which wires up a oneshot reply channel, sends the
message, and awaits the result.

```rust
// Async git subprocess (fetch, push, merge-tree, commit-tree) — runs through
// the CommandStack so the GitHub auth token is attached:
git.request(|reply| GitActorMessage::RunGitCommand { args: vec!["fetch", url], reply }).await?;

// Blocking git2 work — serialized on the actor's dedicated thread:
git.request(|reply| GitActorMessage::PrepareMerge { repo_url, head_sha, reply }).await?;
git.request(|reply| GitActorMessage::UnapplyMerge { remote_name, head_commit_id, main_oid, merge_oid, reply }).await?;
```

`RunGitCommand` is handled async (it awaits the `CommandStack`); all other
variants (`Track`, `ListTrackedRemotes`, `FindRemoteByUrl`, `PrepareMerge`,
`UnapplyMerge`) are git2 work offloaded to `spawn_blocking`. A fresh
`Transaction` is opened per blocking message.

`GithubApiConnection::from_middleware(middleware, api_url)` creates the API client
from a `GithubAuthMiddleware`; `api_url` overrides the GraphQL endpoint (tests pass
the mock's `/graphql` URL).

### Integration tests

Tests live in `cq/josh-cq-tests/tests/merge_queue_tests.rs`. They use `GithubSim`
(Git HTTP + GraphQL) and exercise the full merge queue flow through the real HTTP
webhook path:

| Test | Scenario |
|------|----------|
| `merge_single_pr` | PR with approving review and no checks → merged |
| `pr_not_admissible_without_review` | PR with no approving review → not merged |
| `pr_not_admissible_with_failing_check` | PR with failing required check → not merged |
| `pr_removed_on_close_webhook` | PR closed via webhook → not merged |

Tests return `anyhow::Result<()>` and use `?` for error propagation. The test harness
(`test_helpers.rs`):
1. Creates a temporary bare metarepo with an initial commit on `refs/heads/main`
2. Loads the sled cache and calls `handle_init` to initialize the metarepo
3. Builds a URL → (owner, name) mapping so non-GitHub sim URLs can be resolved
4. Creates a `GithubAuthMiddleware` with a dummy token, a `CommandStack`, and spawns
   the git actor via `spawn_git_actor`
5. Calls `spawn_serve_task` with a long tick interval (ticks driven manually)
   and the sim's GraphQL URL
6. Starts the CQ HTTP server (`bind_router`) so webhooks go through the real HTTP path
7. Wires `GithubSim`'s webhook URL to the CQ server, so sim mutations POST webhooks
8. Drives operations through `SimRepo` (obtained via `github_sim.repo_by_name(owner, name)`)
9. Sends ticks via `harness.tick()` — sends `CqEvent::Tick { done: Some(tx) }` and
   awaits the oneshot, ensuring the full queue cycle completes before the test proceeds

```rust
let repo = harness.github_sim.repo_by_name(owner, name);
let (pr_node_id, number) = repo
    .pr_open("Test PR", "refs/heads/feature", "refs/heads/main")
    .await?;
repo.add_review(number, "maintainer1", ReviewState::Approved).await?;
repo.add_maintainer("maintainer1").await?;
harness.tick().await?;
assert_eq!(repo.pr_by_node_id(&pr_node_id), Some(PrStatus::Closed));
```

Run with:
```
cargo test -p josh-cq-tests --test merge_queue_tests -- --test-threads=1
```

## Test components (`cq/josh-cq-test-components`)

Clean test infrastructure prototyping a replacement for the hand-rolled
`josh-test-github` crate. Motivation: the existing test harness mixes
`Arc<Mutex<…>>` locks with filesystem state and runs `git-http-backend`
in a separate axum task, creating race conditions between programmatic
API calls and HTTP git operations (clone, push).

This crate uses **actor architecture** to eliminate those races without
locks — you can't place a mutex on the filesystem.

### Actor design

All operations go through a single `mpsc::UnboundedSender<ActorMsg>` channel
into an async tokio task. Messages carry a `oneshot::Sender` for the response.

```
HTTP request → axum handler → tx.send(ServeGitHttp{req, resp_ch}) ─┐
User API     → tx.send(Commit{…}) ──────────────────────────────────┤
                                                        mpsc channel │
                                                                     ▼
                                                            actor task (serial)
                                                rx.recv().await
                                                  Commit       → spawn_blocking(do_commit)
                                                  CreateBranch → spawn_blocking(do_create_branch)
                                                  GetHead      → spawn_blocking(do_get_head)
                                                  ServeGitHttp → serve_git_http().await
```

**Why serve HTTP through the actor**: `git-http-backend` is spawned inside the actor
loop, so HTTP clone/push requests are serialized against programmatic commits and
branch creation. No two operations touch the on-disk repository concurrently.

### Key design decisions

- **`git2::Repository` is `!Send`** — opened fresh in each `spawn_blocking` closure
  via `git2::Repository::open(&repo_path)`. The overhead is negligible.
- **`TempDir` is `!Sync`** — wrapped in `Arc<std::sync::Mutex<TestRepoResources>>`
  so `TestRepo` implements `Send + Sync` and can be used in `Arc<TestRepo>` from tests.
  The mutex is never contended; it exists only as a lifetime container.
- **git2 ops run in `spawn_blocking`** — git2 is blocking C code; offloading to the
  blocking thread pool keeps the async runtime responsive.
- **Nested helper `send_response` / `send_join_result`** — generic oneshot send
  with `tracing::error!` if the receiver has been dropped (actor closed).
- **`TreeMode` carries entries** — `Overlay(Vec<TreeEntry>)` / `Replace(Vec<TreeEntry>)`,
  no separate entries parameter.

### File structure

```
cq/josh-cq-test-components/
├── Cargo.toml
├── src/
│   ├── lib.rs          # pub mod exports
│   ├── repo.rs         # TestRepo struct, async API, axum server + handler
│   ├── actor.rs        # ActorMsg enum, run_actor loop, git2 helpers (do_commit, etc.)
│   └── git_http.rs     # prepare_command + serve: CGI git-http-backend subprocess
└── tests/
    └── test_repo_tests.rs  # 9 integration tests
```

### Public API

```rust
pub struct TreeEntry { pub path: String, pub content: String }
pub enum TreeMode { Overlay(Vec<TreeEntry>), Replace(Vec<TreeEntry>) }
pub struct TestRepoResources { /* TempDir guard + actor/server join handles */ }

impl TestRepo {
    pub async fn new() -> anyhow::Result<Self>  // creates bare repo, starts actor + axum
    pub fn path(&self) -> &Path
    pub fn url(&self) -> &Url                    // http://127.0.0.1:PORT/
    pub async fn commit(&self, mode, message, branch_ref) -> Result<Oid>
    pub async fn create_branch(&self, name, from_ref) -> Result<Oid>
    pub async fn get_head(&self, branch_ref) -> Result<Oid>
    pub fn into_parts(self) -> (PathBuf, Arc<Mutex<TestRepoResources>>)
        // Consumes TestRepo, returns the on-disk path + lifetime guard.
        // Used by GithubSim to take over the repo after construction.
}
```

- `TreeMode::Overlay` — build tree from parent's tree + new entries (preserves existing)
- `TreeMode::Replace` — build tree from scratch with only the given entries
- `branch_ref` uses full ref format (`"refs/heads/main"`)
- `create_branch` accepts any revspec (ref name or OID) for `from_ref`

### Running tests

```
cargo test -p josh-cq-test-components
```

Tests use `anyhow::Result<()>` with `?` propagation. Git CLI calls are wrapped in
`spawn_blocking` to avoid blocking tokio worker threads during HTTP tests.

## Axum patterns (0.8.x)

### Extractors

**In axum handlers**, use extractors as function parameters:

```rust
async fn handler(State(tx): State<Sender>, Json(payload): Json<MyType>) -> Response { ... }
async fn handler(req: axum::extract::Request) -> Response { ... }  // full request, body intact
```

**Outside axum handlers** (e.g., in an actor receiving a `Request`), use `FromRequest` directly:

```rust
use axum::extract::FromRequest;
use axum::Json;

let Json(payload) = Json::<MyType>::from_request(request, &()).await?;
```

This requires `S: Send + Sync` on the state — `()` works as a no-op state. The `RequestExt::extract()` trait method exists but is fragile because `Json<T>` implements `FromRequest<(), ViaRequest>` not `FromRequest<(), Body>`, making turbofish awkward. Direct `FromRequest::from_request()` is more reliable.

### Responses

**Preferred**: implement `IntoResponse` instead of manually calling `Response::builder()`:

```rust
use axum::response::{IntoResponse, Response};
use axum::Json;

impl IntoResponse for MyError {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(serde_json::json!({"errors": [...]}))).into_response()
    }
}
```

The `(StatusCode, Json(body))` tuple already implements `IntoResponse` — composing via `into_response()` handles headers and serialization. Never build `Response<Body>` manually with `.header("Content-Type", ...)` when `Json(...)` does it for you.

## Juniper patterns (0.17.x)

### Schema construction

```rust
use juniper::{RootNode, EmptyMutation, EmptySubscription, graphql_object};

struct Query;
#[graphql_object(context = Context)]
impl Query {
    async fn field(&self, arg: String, context: &Context) -> Option<ReturnType> { ... }
}

type Schema = RootNode<Query, EmptyMutation<Context>, EmptySubscription<Context>>;
fn schema() -> Schema { Schema::new(Query, EmptyMutation::new(), EmptySubscription::new()) }
```

- `RootNode` has **no lifetime parameter** in 0.17 (was `RootNode<'static, ...>` in older versions).
- Resolvers can be `async fn` — Juniper 0.17 supports both sync and async, use `juniper::execute().await`.
- Returning `Option<T>` from a resolver makes the field nullable; returning `None` produces `null` with no error — proper GraphQL semantics for missing resources.
- `#[graphql_object]` serializes fields in **method definition order**.

### Context

```rust
struct Context { /* any data */ }
impl juniper::Context for Context {}
```

Passed as `&Context` to all resolvers via `juniper::execute(query, op_name, &schema, &variables, &context).await`.

### Response serialization

Use `juniper::http::GraphQLResponse` (the `http` module is always available, no feature needed):

```rust
let result = juniper::execute(query, op_name, &schema, &variables, &context).await;
let response = juniper::http::GraphQLResponse::from_result(result);
(StatusCode::OK, Json(response)).into_response()
```

`GraphQLResponse` implements `Serialize` producing `{"data": ...}` on success and `{"errors": [{"message": ..., "locations": [...], "path": [...]}]}` on error. No manual JSON construction needed.

### Variable conversion

Juniper's `execute()` takes `&HashMap<String, InputValue<S>>`. To convert from `serde_json::Value` without pulling in the `http` feature:

```rust
use juniper::{InputValue, DefaultScalarValue};
use indexmap::IndexMap;  // InputValue::object() takes IndexMap, not HashMap

fn to_input_value(json: &serde_json::Value) -> InputValue<DefaultScalarValue> { ... }
```

`InputValue` has constructors: `scalar(v)`, `list(vec)`, `object(IndexMap)`, `Null`.

## GitHub simulator (`forges/josh-github-sim`)

Actor-based simulated GitHub environment — Git HTTP + GraphQL API. Motivation: the
existing `josh-test-github` crate runs git-http-backend in a separate axum task,
creating filesystem races with programmatic repo operations. A unified actor
eliminates those races. Juniper provides proper GraphQL query parsing, field
selection, and error formatting (the old `GraphQLMock` used hand-rolled JSON dispatch
by `operationName`).

### Actor design

Single `mpsc` channel serializes git-http, GraphQL, and PR lifecycle operations:

```
Git HTTP   → handle_git     → tx.send(ServeGitHttp{owner,name,req}) ─┐
GraphQL    → handle_graphql → tx.send(GraphQLRequest{req}) ──────────┤
SimRepo    → pr_open() etc  → tx.send(PrOpen{…}) / PrClose / AddReview / … ─┤
                                                            mpsc      │
                                                                      ▼
                                                              actor task (serial)
                                                       ServeGitHttp → git_http::serve()
                                                       GraphQLReq   → juniper::execute().await
                                                       PrOpen       → resolve OIDs, gen IDs, store, webhook
                                                       PrClose/AddReview/… → mutate state, webhook
```

GraphQL requests carry the full `axum::extract::Request` — the actor extracts the
body via `Json::<GraphQLPayload>::from_request()` and executes against a Juniper
schema. Git HTTP requests are routed by `owner/name` prefix stripped from the URL path.

### Multi-repo routing

`GithubSim::new()` accepts `Vec<RepoConfig>`, each containing a pre-prepared
`TestRepo` + `owner`/`name` metadata. The `TestRepo` is consumed via `into_parts()`
— its old actor/server are shut down, only the on-disk repo (and its `TempDir` guard)
survive. All further interaction is via the Git HTTP URL (`/owner/name/...`) or
GraphQL endpoint (`/graphql`). Inner repos are never exposed.

### Key design decisions

- **`TestRepo` consumed on construction** — `into_parts()` drops the old actor +
  server, keeps the on-disk path + `TempDir` guard. The unified actor takes over.
- **Juniper schema** — proper GraphQL types (`Query`, `Repository`, `DefaultBranchRef`)
  with async resolvers for future inter-actor communication (resolver sends message
  to inner actor, awaits response).
- **`juniper::http::GraphQLResponse`** — Juniper's built-in response type; serializes
  to `{"data": ...}` / `{"errors": [...]}` with proper `locations` and `path` fields.
- **`GraphQLError::from_message`** — thin `IntoResponse` type for extractor failures
  (Juniper's `GraphQLError` enum has no simple string constructor).
- **No `http` feature needed** — Juniper's `http` module is always compiled.
  `InputValue` conversion from `serde_json::Value` is manual (~20 lines).

### File structure

```
forges/josh-github-sim/
├── Cargo.toml
├── src/
│   ├── lib.rs          # pub use sim::{GithubSim, RepoConfig, SimRepo, PrStatus}
│   │                   # pub use graphql::{GraphQLState, MockPr, MockRuleset, ReviewState, RuleEnforcement}
│   ├── sim.rs          # GithubSim, SimRepo, RepoConfig, PrStatus, axum server
│   ├── actor.rs        # ActorMsg enum, run_actor loop
│   └── graphql/
│       ├── mod.rs      # Juniper schema, handle_graphql_request
│       ├── types.rs    # MockPr, MockRuleset, RepoState, GraphQLState, ReviewState
│       ├── webhooks.rs # Webhook payload builders + HTTP POST
│       ├── query.rs    # GraphQL Query root
│       ├── mutation.rs # GraphQL Mutation root (closePullRequest, addComment)
│       ├── repository.rs
│       ├── pull_request.rs
│       ├── context.rs
│       ├── collaborator.rs
│       ├── git_object.rs
│       └── ruleset.rs
└── tests/
    └── integration.rs  # 5 integration tests
```

### Public API

```rust
pub struct RepoConfig { pub owner: String, pub name: String, pub repo: TestRepo }

// GithubSim — factory + cross-repo accessors
impl GithubSim {
    pub async fn new(repos: Vec<RepoConfig>) -> anyhow::Result<Self>
    pub fn url(&self) -> &Url
    pub fn graphql_url(&self) -> &Url
    pub fn graphql_state(&self) -> &Arc<Mutex<GraphQLState>>
    pub fn set_webhook_url(&self, url: Url)
    pub fn repo_by_name(&self, owner: &str, name: &str) -> SimRepo
}

// SimRepo — per-repo operations (obtained from GithubSim)
impl SimRepo {
    pub fn owner(&self) -> &str
    pub fn name(&self) -> &str
    pub async fn pr_open(&self, title, head_ref_name, base_ref_name) -> Result<(String, i64)>
    pub async fn pr_close(&self, node_id: &str) -> Result<()>
    pub async fn add_review(&self, pr_number, reviewer, state: ReviewState) -> Result<()>
    pub async fn add_maintainer(&self, login: &str) -> Result<()>
    pub async fn add_ruleset(&self, ruleset: MockRuleset) -> Result<()>
    pub async fn complete_check_run(&self, check_name, pr_number, conclusion) -> Result<()>
    pub fn pr_by_node_id(&self, node_id: &str) -> Option<PrStatus>
    pub fn pr_comments_by_node_id(&self, node_id: &str) -> Vec<String>
}

pub enum PrStatus { Open, Closed }
pub enum ReviewState { Approved, ChangesRequested, Commented, Dismissed }
pub enum RuleEnforcement { Active, Disabled, Evaluate }
pub struct MockRuleset { pub id, name, enforcement, include_refs, exclude_refs, required_checks }
pub struct MockPr { pub node_id, number, title, head_sha, base_sha, base_branch }
```

- `pr_open` auto-generates node_id and PR number, resolves ref names to OIDs via git2.
- `pr_close`, `add_review`, etc. emit webhooks to the configured `webhook_url`.
- `pr_by_node_id` and `pr_comments_by_node_id` are synchronous in-memory lookups.

### Running tests

```
cargo test -p josh-github-sim
```

Tests use `anyhow::Result<()>`, `insta::assert_json_snapshot!` for GraphQL responses,
and `spawn_blocking` for git CLI calls.
