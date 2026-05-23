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
| `josh-test-github` | Simulated GitHub environment: `SimRepo`, `GraphQLMock`, `GitServer`, `TestRepo` |

## Merge queue (`cq/`)

Both the merge queue library/binary and its integration tests live under `cq/`:

```
cq/
├── josh-cq/          # library + binary
│   ├── src/
│   │   ├── bin/josh-cq.rs   # CLI entry point
│   │   ├── lib.rs           # module declarations
│   │   ├── types.rs         # CqEvent, TrackRequest, UserAction
│   │   ├── models.rs        # CandidatePr, CqActorState (+ methods)
│   │   ├── server.rs        # HTTP router, spawn_serve_task (actor loop)
│   │   ├── fetch.rs         # handle_fetch, API helpers
│   │   ├── step.rs          # handle_step, select_candidate, run_queue_cycle
│   │   ├── admission.rs     # process_pr_review, process_check_run
│   │   ├── webhook.rs       # handle_webhook
│   │   ├── track.rs         # handle_track
│   │   ├── init.rs          # handle_init
│   │   └── remote.rs        # list_refs (git ls-remote)
│   └── Cargo.toml
│
└── josh-cq-tests/   # integration tests
    ├── src/lib.rs
    ├── tests/merge_queue_tests.rs
    └── Cargo.toml
```

### Subcommands

The CQ binary (`josh-cq`) supports:
- **`init`** — creates an empty metarepo commit on HEAD
- **`serve`** — starts HTTP server receiving webhooks and track requests
- **`track`** — adds a remote as a tracked repo (git ls-remote + `.link.josh` + `refs.json`)

### Architecture

The `serve` event loop uses an **actor model**. All input — webhook events, track
requests, and periodic polling ticks — is sent through a single `mpsc` channel.
A single `spawn_blocking` task processes events serially, mutating
`CqActorState` without locks. No `state.clone()` — handler functions take
`&mut CqActorState`.

The queue cycle: **Fetch** → **Evaluate** → **Step** (merge) → repeat while
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
                              Fetch (on Tick only)
                                    │
                                    ▼
                        run_queue_cycle (Tick + Webhook)
                         Evaluate → Step → loop
```

- **Tick** (every 10 min) triggers `handle_fetch` (discovers open PRs from GitHub
  GraphQL) then `run_queue_cycle`. Catches PRs missed by webhook delivery failures.
- **Webhooks** update admission state and candidate list immediately, then fall
  through to `run_queue_cycle`. No fetch on webhook — only admission updates.
- **Track** adds a new remote to the metarepo and is handled inline. Does not trigger
  the queue cycle.
- **Merge** happens in the metarepo via `git merge-tree --write-tree` +
  `git commit-tree`, then pushes to the remote's main branch and closes the PR on
  GitHub with a "Merged by Josh merge queue" comment.

### Actor loop (server.rs)

```rust
tokio::task::spawn_blocking(move || {
    let mut state = CqActorState { url_owner_map, ..Default::default() };
    while let Some(event) = event_rx.blocking_recv() {
        let transaction = /* open */;
        match event {
            CqEvent::Tick => {
                // Fetch open PRs, then fall through to evaluate→step
                if let Err(e) = handle_fetch(&transaction, api.as_deref(), &mut state) { ... }
            }
            CqEvent::Track(req) => {
                // Add remote to metarepo; no merge needed
                handle_track(...);
            }
            CqEvent::Webhook(payload) => {
                // Update admission state, then fall through to evaluate→step
                if let Err(e) = handle_webhook(&payload, &transaction, api.as_deref(), &mut state) { ... }
            }
        }
        run_queue_cycle(&mut state, &transaction, api.as_deref());
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

Methods on `CqActorState`:
- `resolve_owner_repo(&self, url) -> Option<(String, String)>` — try `parse_owner_repo`, fall back to `url_owner_map`
- `get_or_fetch_admission(&mut self, url, api)` — lazy-populate required checks
- `get_or_init_pr_admission(&mut self, node_id, url, api)` — init `AdmissionState` for a PR
- `upsert_candidate`, `remove_candidate`, `get_candidate`

`AdmissionState` (in `josh-github-changes`):
- `required_checks`: `BTreeMap<RequiredStatusCheck, bool>` — check name → passed
- `maintainer_reviews`: `BTreeMap<String, PullRequestReviewState>` — login → review state
- `maintainers`: `HashSet<String>` — users with write access
- `admissible()`: ≥1 maintainer approved, no changes requested, all required checks passed

### API calls from sync context

The actor runs in `spawn_blocking` (sync context). All GraphQL calls go through:
```rust
tokio::runtime::Handle::current().block_on(api.some_method(...))
```

`GithubApiConnection::from_environment()` resolves credentials from `GH_TOKEN` env var
or the stored device-flow token. `GithubApiConnection::for_test(url)` creates an
unauthenticated client pointing at a mock GraphQL server.

### Integration tests

Tests live in `cq/josh-cq-tests/tests/merge_queue_tests.rs`. They use a simulated
GitHub environment (`SimRepo` + `GraphQLMock` + git HTTP remote) and exercise the
full merge queue flow:

| Test | Scenario |
|------|----------|
| `merge_single_pr` | PR with approving review and no checks → merged |
| `pr_not_admissible_without_review` | PR with no approving review → not merged |
| `pr_not_admissible_with_failing_check` | PR with failing required check → not merged |
| `pr_removed_on_close_webhook` | PR closed via webhook → not merged |

Tests return `anyhow::Result<()>` and use `?` for error propagation. The test harness:
1. Creates a temporary bare repo as the metarepo
2. Initializes the CQ actor with a long tick interval (ticks driven manually)
3. Sends `CqEvent::Webhook(...)` and `CqEvent::Tick` through the event channel
4. Polls with `poll_until` to wait for expected side effects (PR closed, comment posted)

Run with:
```
cargo test -p josh-cq-tests --test merge_queue_tests -- --test-threads=1
```
