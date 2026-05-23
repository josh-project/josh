# Merge Queue Refactoring Tasks

## High Priority

### 1. Split `josh-cq/src/state.rs` into focused modules

`state.rs` (~560 lines) mixes data models, API helpers, business logic, and git utilities
with zero separation of concerns. Split into at least:

| New module | Contents |
|---|---|
| `src/models.rs` | `CandidatePr`, `CqActorState`, `AdmissionRelevantEvent`, `resolve_owner_repo` → method on `CqActorState` |
| `src/fetch.rs` | `handle_fetch`, `fetch_required_checks`, `fetch_maintainers`, `lookup_open_prs_by_sha` |
| `src/step.rs` | `handle_step`, `select_candidate`, `run_queue_cycle`, `spawn_git_command_stdout` |
| `src/admission.rs` | `process_admission_events` |

### 2. Eliminate `state.clone()` on every actor event

`server.rs:110,126,130` clones the entire `CqActorState` before passing to
`handle_fetch` / `handle_webhook`. These functions take `state` by value and return
`CqActorState` purely to preserve the old state on error. Change them to take
`&mut CqActorState` — they already mutate in-place; early-return-on-error is enough
to avoid corrupting state.

Once done, remove the `Clone` derive from `CqActorState`.

### 3. Move `spawn_git_command_stdout` into `josh_core::git`

`state.rs:398-419` duplicates the git process spawning logic from
`josh_core::git::spawn_git_command` but captures stdout. Consolidate into
`josh_core::git` as `spawn_git_command_stdout` (or add an output-capture parameter
to the existing function). Remove the private copy from `state.rs` / the new
`step.rs`.

### 4. Fix UB: `unsafe { std::env::set_var }` in multi-threaded tests

`merge_queue_tests.rs:73` calls `set_var` inside `start_test_harness`, which runs
in a `multi_thread` tokio runtime with `worker_threads = 10`. `set_var` is not
thread-safe — Rust marks it `unsafe` for this reason. Use `std::sync::Once` to
set it once before any test runs.

### 5. Deduplicate test helper functions across crates

`make_pr_node_id`, `make_repository`, `make_pr_payload` exist identically in both:
- `forges/josh-test-github/src/sim_repo.rs:109-145` (private)
- `josh-test-cq/tests/merge_queue_tests.rs:17-53` (private)

Make the `sim_repo` versions `pub`, export them from `josh-test-github`, and
import them in the test file. Delete the copies.

---

## Medium Priority

### 6. Split `handle_step` into smaller functions

`handle_step` (state.rs:423-538, ~116 lines) does too many things in one function.
Extract:

- `compute_merge(repo, main_sha, head_sha)` → `Result<String>` (merged tree OID)
- `push_merge(repo, merge_commit, candidate)` → `Result<()>`
- `post_merge_close(api, candidate, merge_commit)` → `Result<()>`

### 7. Avoid cloning `CandidatePr` on every `select_candidate` call

`run_queue_cycle` calls `select_candidate` in a loop — each iteration clones all
10 `String` fields, then `handle_step` uses the original reference. The clone is
immediately dropped. Return `&CandidatePr` instead, or wrap candidates in `Arc`.

### 8. Make `resolve_owner_repo` a method on `CqActorState`

Every call site passes `&state.url_owner_map`:

```rust
resolve_owner_repo(clone_url, &state.url_owner_map)
```

Make it `fn resolve_owner_repo(&self, url: &str) -> Option<(String, String)>` on
`CqActorState`. This is blocked by task 1 (model extraction).

---

## Low Priority

### 9. Simplify duplicate `handle_get` / `handle_post` in `git_server.rs`

Both handlers are identical one-liners delegating to `serve_git`. Register the
same function for both methods:

```rust
.route("/{*path}", get(handle_git).post(handle_git))
```

Or use `axum::routing::any`.

### 10. Simplify `AdmissionRelevantEvent` pattern

`AdmissionRelevantEvent` is `Copy` but only stores references. The name suggests
it IS an event, not a reference to one. Options:
- Rename to `AdmissionEventRef`
- Or inline the match in `process_admission_events` to take the raw webhook types
  directly, avoiding the intermediate `Vec<(String, AdmissionRelevantEvent)>`
  allocation in `webhook.rs`.

### 11. Add comments documenting Tick vs Webhook asymmetry in the actor loop

`CqEvent::Tick` only triggers `handle_fetch` (discovers PRs from GitHub), while
`CqEvent::Webhook` handles admission updates and candidate management. Both fall
through to `run_queue_cycle`. This asymmetry is by design but easy to miss when
reading the `match` blocks. Add a short comment explaining the flow.

### 12. Consolidate GraphQL mock locks into a single `Mutex<GraphQLStateInner>`

`graphql_mock.rs` uses 7 separate `Mutex` fields (`prs`, `reviews`, `maintainers`,
`rulesets`, `required_checks`, `closed_prs`, `comments`). If any handler panics
while holding one lock, that specific mutex is poisoned and all subsequent
requests to that data path crash. For test code a single `Mutex<GraphQLStateInner>`
would be simpler and avoid the lock-per-field overhead.

### 13. `CandidatePr` has 10 owned `String` fields

All 10 fields are cloned eagerly on every `upsert_candidate` and `select_candidate`
call. Consider using `Arc<str>` for the fields or wrapping the whole struct in
`Arc<CandidatePr>` in the candidates map. This is more impactful if the candidate
pool is expected to be large.
