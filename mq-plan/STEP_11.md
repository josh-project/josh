# Step 11: Split `cq.rs` into modules

## Why

After step 10, `josh-cq/src/cq.rs` will be ~600+ lines mixing types, actor state,
webhook handling, CLI commands, HTTP routing, and the server loop. Splitting into
focused modules makes each concern easier to navigate and test independently.

## What to change

### File: `josh-cq/src/types.rs` (new)

Move pure data types with no behavior:

```rust
// Move from cq.rs:
// - TrackRequest
// - CqEvent
// - UserAction
// - AdmissionRelevantEvent
```

No logic changes — just cut from `cq.rs` and paste into `types.rs`.

### File: `josh-cq/src/state.rs` (new)

Move actor state and its helpers:

```rust
// Move from cq.rs:
// - CqActorState struct + impl (get_or_fetch_admission, get_or_init_pr_admission)
// - fetch_maintainers (free function)
// - fetch_required_checks (free function)
// - lookup_open_prs_by_sha (free function)
// - CandidatePr (added in step 5)
// - handle_fetch (added in step 7)
// - candidate evaluation (added in step 8)
// - handle_step (added in step 9)
```

This module owns the merge queue's core state machine. Keep the free functions
module-private where possible (`pub(crate)` or bare).

### File: `josh-cq/src/init.rs` (new)

```rust
// Move from cq.rs:
// - handle_init
```

Depends only on `josh-core` and `josh-link` — no actor state, no GitHub API.

### File: `josh-cq/src/track.rs` (new)

```rust
// Move from cq.rs:
// - handle_track
// - default_mode
```

Depends on `josh-core`, `josh-link`, `remote.rs`. No actor state.

### File: `josh-cq/src/webhook.rs` (new)

```rust
// Move from cq.rs:
// - handle_webhook
// - webhook_repository
```

Depends on `types.rs`, `state.rs`, `josh-github-webhooks`.

### File: `josh-cq/src/server.rs` (new)

```rust
// Move from cq.rs:
// - make_router
// - spawn_serve_task
// - handle_action
// - enqueue
// - track_handler
// - webhook_handler
```

Thin glue: router setup + the actor loop. Depends on `types.rs`, `init.rs`,
`track.rs`, `webhook.rs`, `state.rs`.

### File: `josh-cq/src/lib.rs`

Replace the single `pub mod cq;` with:

```rust
pub mod init;
pub mod remote;
pub mod server;
pub mod state;
pub mod track;
pub mod types;
pub mod webhook;
```

### File: `josh-cq/src/cq.rs`

**Delete this file.** All contents have moved to the modules above.

### Imports

Update `use` paths in all moved modules. The `bin/` entry point will import from
the re-exported modules instead of `cq::`. Check `josh-cq/src/bin/` for any
`use josh_cq::cq::` paths and update them.

### Acceptance

- `cargo build -p josh-cq` succeeds
- `cargo fmt` passes
- `cargo clippy -p josh-cq` passes (no new warnings)
- No `pub mod cq;` remains in `lib.rs`
- All existing functionality is preserved — this is purely a code move, no
  behavior changes
