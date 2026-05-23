# Shared context for merge queue refactoring subagents

## Project

Josh is a git monorepo proxy. The merge queue lives in the `josh-cq` crate.

## Conventions

- **Always run `cargo fmt` before committing.**
- **One commit per task.** Follow existing commit style: short imperative subject line.
  See `git log` for examples.
- **Read before editing.** Always read a file before using Edit on it.
- **Stay focused.** Only implement what the current task describes. Don't drift into
  unrelated refactoring.
- **Follow existing patterns.** Copy the style of adjacent code — same error handling,
  same logging approach (`tracing::info!` / `tracing::error!`), same import grouping.
- **Verify compilation.** Run `cargo build` before committing, and `cargo test` if
  the task touches test code. For the merge queue specifically, run:
  ```
  cargo test -p josh-test-cq --test merge_queue_tests -- --test-threads=1
  ```

## Merge queue architecture

The `serve` event loop uses an **actor model**. All input — webhook events, track
requests, and periodic polling ticks — is sent through a single `mpsc` channel.
A single `spawn_blocking` task processes events serially, mutating `CqActorState`
without locks.

The queue cycle: **Fetch** → **Evaluate** → **Step** (merge) → repeat while
admissible PRs remain.

### Key files in `josh-cq/src/`

| File | Contents |
|---|---|
| `lib.rs` | Module declarations |
| `types.rs` | `CqEvent`, `TrackRequest`, `AdmissionRelevantEvent`, `UserAction`, `GH_TOKEN_ENV` |
| `state.rs` | `CandidatePr`, `CqActorState`, `handle_fetch`, `handle_step`, `run_queue_cycle`, `select_candidate`, `process_admission_events`, `spawn_git_command_stdout`, API helpers |
| `server.rs` | HTTP router (`/v1/track`, `/v1/webhook`), `spawn_serve_task` (actor loop + tick timer) |
| `webhook.rs` | `handle_webhook` — deserializes webhook payloads, updates state |
| `track.rs` | `handle_track` — adds a remote to the metarepo |
| `init.rs` | `handle_init` — creates initial metarepo commit |
| `remote.rs` | `list_refs` — `git ls-remote` wrapper |
| `bin/josh-cq.rs` | CLI entry point |

### Key files in `josh-test-github/src/`

| File | Contents |
|---|---|
| `lib.rs` | Module declarations |
| `sim_repo.rs` | `SimRepo` — simulated GitHub repo with hook listener |
| `graphql_mock.rs` | `GraphQLMock` — mock GraphQL server with state tracking |
| `git_server.rs` | `GitServer` — `git http-backend` over HTTP |
| `test_repo.rs` | `TestRepo` — bare git repo wrapper for tests |
| `webhook_sender.rs` | `send_webhook` — POST webhook to CQ |

### Key files in `josh-test-cq/`

| File | Contents |
|---|---|
| `tests/merge_queue_tests.rs` | Integration tests (4 test cases) |
| `src/lib.rs` | Crate-level doc comment |

### Other relevant crates

| Crate | What it provides |
|---|---|
| `josh-core` | `CacheStack`, `TransactionContext`, `Transaction`, `find_link_files`, `spawn_git_command`, `LinkMode` |
| `josh-link` | `make_signature`, `update_links`, `prepare_link_add` |
| `josh-github-graphql` | `GithubApiConnection`, generated query types |
| `josh-github-changes` | `AdmissionState`, `parse_owner_repo` |
| `josh-github-webhooks` | `WebhookPayload`, webhook type definitions |

### Data flow

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
                              Fetch (on Tick)
                                    │
                                    ▼
                              Evaluate candidates
                                    │
                                    ▼
                              Step (merge admissible PRs)
                                    │
                                    ▼
                              Loop (while more PRs are admissible)
```

### Actor loop (from `server.rs`)

```rust
tokio::task::spawn_blocking(move || {
    let mut state = CqActorState { url_owner_map, ..Default::default() };

    while let Some(event) = event_rx.blocking_recv() {
        let transaction = /* open transaction */;

        match event {
            CqEvent::Tick => {
                state = match handle_fetch(&transaction, api.as_deref(), state.clone()) { ... };
            }
            CqEvent::Track(req) => { handle_track(...); }
            CqEvent::Webhook(payload) => {
                state = match handle_webhook(&payload, &transaction, api.as_deref(), state.clone()) { ... };
            }
        }

        run_queue_cycle(&mut state, &transaction, api.as_deref());
    }
});
```

## API calls from sync context

The actor runs in `spawn_blocking` (sync context), but `GithubApiConnection` methods
are `async`. All GraphQL calls go through:
```rust
tokio::runtime::Handle::current().block_on(api.some_method(...))
```

## Tests

Integration tests in `josh-test-cq/tests/merge_queue_tests.rs`:
- Use `#[tokio::test(flavor = "multi_thread", worker_threads = 10)]`
- Create a `SimRepo` + `GraphQLMock` + `GithubApiConnection::for_test()`
- Start the CQ actor via `spawn_serve_task` with a long tick interval
- Drive the queue manually by sending `CqEvent::Webhook(...)` and `CqEvent::Tick`
  through the event channel
- Poll with `poll_until` to wait for expected side effects

Run with:
```
cargo test -p josh-test-cq --test merge_queue_tests -- --test-threads=1
```
