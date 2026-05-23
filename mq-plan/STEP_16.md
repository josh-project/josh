# Step 16: Write integration tests

## Why

With the test infrastructure in place (`SimRepo`, `GraphQLMock`,
`GithubApiConnection::for_test`), we can write end-to-end integration tests that
exercise the full merge queue flow: metarepo setup → webhook delivery → GraphQL
queries → merge commit → PR close + comment.

## What to change

### File: `josh-cq/Cargo.toml`

Add `josh-test-github` as a dev-dependency:

```toml
[dev-dependencies]
josh-test-github.workspace = true
```

### File: `josh-cq/tests/merge_queue_tests.rs` (new)

Integration tests live in `josh-cq/tests/` so they compile as a separate crate
linked against `josh-cq` as a library.

#### Helper: `start_test_harness`

A shared setup function that:

1. Creates a `SimRepo` with initial commit on `main`
2. Creates a `GraphQLMock` configured with the given PRs/reviews/maintainers/checks
3. Starts the mock GraphQL server
4. Creates a metarepo in a temp directory via `josh-cq init`
5. Adds a `.link.josh` file via `josh-cq track` pointing at the sim repo
6. Starts a webhook receiver server (collects events into a channel)
7. Starts `spawn_serve_task` with the mock API connection
8. Returns handles for sending webhooks, reading state, and cleanup

```rust
struct TestHarness {
    event_tx: mpsc::Sender<CqEvent>,
    sim_repo: Arc<SimRepo>,
    graphql_mock: Arc<GraphQLMock>,
    metarepo_path: PathBuf,
    // cleanup on drop
    _cache: Arc<CacheStack>,
}
```

#### Test: `merge_single_pr`

1. Setup: sim repo with main + feature branch, one commit on the branch
2. Configure GraphQLMock: PR listed as open, one approving maintainer review,
   no required checks (empty ruleset — or a ruleset with no checks)
3. Open PR via `sim_repo.open_pr("feature", "main")`
4. Send `Tick` event to trigger the queue cycle
5. Poll: the PR is merged (wait up to 5 seconds)
   - `graphql_mock.closed_pr_node_ids()` contains the PR's node ID
   - `graphql_mock.comments()` contains a "Merged by Josh merge queue" message
6. Assert: `sim_repo.pull_requests()` shows the PR is closed

#### Test: `pr_not_admissible_without_review`

Same setup but no approving review (e.g., only a "commented" review or no
review at all). Assert PR is NOT closed after a tick.

#### Test: `pr_not_admissible_with_failing_check`

Same setup but `required_checks = ["ci/test"]` and the check run conclusion is
`Failure`. Assert PR is NOT closed.

#### Test: `merge_updates_base_sha_on_push`

1. Open a PR
2. Push a new commit to `main` (simulating another PR merged ahead)
3. Send a `Push` webhook
4. Verify the candidate's `base_sha` was updated (indirectly via merge success
   after the push)

#### Test: `pr_removed_on_close_webhook`

1. Open a PR
2. Send a `PullRequestEvent::Closed` webhook
3. Verify the candidate is removed from the pool (no merge attempted on next tick)

### Implementation notes

- Use `tokio::test` with `flavor = "multi_thread"` since `spawn_serve_task` uses
  `spawn_blocking`
- The tick timer skips the first tick (waits one full interval). To trigger a
  cycle immediately, send a `CqEvent::Tick` directly via the event channel
- For poll-style assertions, use a short timeout loop (e.g., check every 100ms
  for up to 5 seconds) rather than `sleep` + one check
- Each test must create its own temp directories that clean up on drop

### Acceptance

- `cargo test -p josh-cq --test merge_queue_tests` — all tests pass
- `cargo fmt` passes
