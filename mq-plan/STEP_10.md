# Step 10: Wire actor loop with tick timer into `serve`

## Why

The merge queue needs to run automatically. Using an actor model:
- A single `spawn_blocking` task processes events serially through an mpsc channel.
- A background timer sends `CqEvent::Tick` every 10 minutes as a fallback.
- Webhooks also trigger immediate queue cycles (they're events on the same channel).
- After each event, the actor runs evaluate→step to merge any admissible PRs.

All state mutations are serialized — no locks, no races.

## What to change

### File: `josh-cq/src/cq.rs`

#### 1. Add `Tick` variant to `CqEvent`

```rust
pub enum CqEvent {
    Track(TrackRequest),
    Webhook(WebhookPayload),
    /// Periodic polling tick — triggers a full fetch + evaluate + step cycle.
    Tick,
}
```

#### 2. Add a helper to run the queue cycle

Extract the evaluate→step loop so it can be called after each event:

```rust
/// Run evaluate→step while admissible PRs remain.
/// Called after every event (webhook or tick) to try to make progress.
fn run_queue_cycle(
    state: &mut CqActorState,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
) {
    loop {
        let candidate = match select_candidate(state) {
            Some(c) => c,
            None => {
                tracing::debug!("no admissible PRs");
                break;
            }
        };

        match handle_step(&candidate, transaction, api, state) {
            Ok(()) => {
                tracing::info!(
                    pr = %candidate.node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "merged PR"
                );
            }
            Err(e) => {
                tracing::error!(
                    pr = %candidate.node_id,
                    number = candidate.number,
                    error = ?e,
                    "failed to merge PR; will retry next cycle"
                );
                break;
            }
        }
    }
}
```

#### 3. Rewrite `spawn_serve_task`

```rust
pub fn spawn_serve_task(
    repo_path: PathBuf,
    cache: Arc<CacheStack>,
    tick_interval_secs: u64,
) -> mpsc::Sender<CqEvent> {
    let (event_tx, event_rx) = mpsc::channel::<CqEvent>(100);

    let api: Option<Arc<GithubApiConnection>> =
        GithubApiConnection::from_environment().map(Arc::new);

    if api.is_none() {
        tracing::warn!(
            "{} not set and no stored credentials found",
            GH_TOKEN_ENV
        );
    }

    // Spawn the periodic tick timer
    let tick_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_secs(tick_interval_secs)
        );
        // Skip the immediate first tick — wait one full interval before
        // the first fetch, giving the server time to start and webhooks
        // to arrive.
        interval.tick().await;
        loop {
            interval.tick().await;
            if tick_tx.send(CqEvent::Tick).await.is_err() {
                break; // channel closed
            }
        }
    });

    // Spawn the actor — serializes all state access
    tokio::task::spawn_blocking(move || {
        let mut state = CqActorState::default();

        while let Some(event) = event_rx.blocking_recv() {
            let transaction = match TransactionContext::new(&repo_path, cache.clone()).open(None) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to open transaction");
                    continue;
                }
            };

            match event {
                CqEvent::Tick => {
                    tracing::info!("tick: running fetch");
                    state = match handle_fetch(&transaction, api.as_deref(), state) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(error = ?e, "fetch failed");
                            continue;
                        }
                    };
                }
                CqEvent::Track(req) => {
                    match handle_track(&req.url, &req.id, &req.mode, &transaction) {
                        Ok(action) => handle_action(action),
                        Err(e) => tracing::error!(error = ?e, "track failed"),
                    };
                }
                CqEvent::Webhook(payload) => {
                    state = match handle_webhook(
                        &payload, &transaction, api.as_deref(), state,
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(error = ?e, "webhook handling error");
                            continue;
                        }
                    };
                }
            }

            // After every event (tick, track, webhook), try to make progress
            run_queue_cycle(&mut state, &transaction, api.as_deref());
        }
    });

    event_tx
}
```

Key points:
- **Tick timer** uses `tokio::spawn` (not `spawn_blocking`) — it's an async sleep,
  not CPU-bound work. It sends `Tick` events into the same mpsc channel.
- **Actor** uses `blocking_recv()` (no timeout) — the timer handles the periodic
  wake-up by sending messages, so no need for `blocking_recv_timeout`.
- **Fetch only on Tick** — webhook events don't re-fetch; they just update
  admission state from the webhook payload, then run evaluate→step. Tick does
  a full fetch to catch missed webhooks.
- **First tick is skipped** (`interval.tick().await` before the loop) so the
  server has time to start before the first full fetch.

### File: `josh-cq/src/bin/josh-cq.rs`

Update `ServeArgs`:

```rust
#[derive(clap::Parser)]
struct ServeArgs {
    #[arg(long, default_value = "8080")]
    port: u16,
    #[arg(long)]
    webhook_relay: Option<String>,
    #[arg(long, env = "JOSH_CQ_WEBHOOK_TOKEN", hide_env_values = true)]
    webhook_relay_token: Option<String>,
    /// Queue tick interval in seconds (default: 600 = 10 minutes)
    #[arg(long, default_value = "600")]
    tick_interval: u64,
}
```

Update the call:

```rust
let event_tx = josh_cq::cq::spawn_serve_task(repo_path, cache, args.tick_interval);
```

### Acceptance

- `cargo build --bin josh-cq` succeeds
- `cargo fmt` passes
- `cargo run --bin josh-cq -- serve --help` shows `--tick-interval` with default 600
- `spawn_serve_task` signature is updated everywhere it's called
- The actor loop calls `run_queue_cycle` after every event
