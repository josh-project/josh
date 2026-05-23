# Step 6: Handle `PullRequest` and `Push` webhook events

## Why

Currently `handle_webhook()` only processes `CheckRun` and `PullRequestReview`
events. We also need to track PR lifecycle events (opened, synchronize, closed)
to keep the candidate pool current, and push events to update base branch SHAs
when the main branch advances.

After `handle_webhook()` returns updated state, the actor loop (Step 10) will
immediately run evaluate→step, so the queue reacts to webhooks in real time.

## What to change

### File: `josh-cq/src/cq.rs`

Restructure `handle_webhook()` to handle all event types. Replace the early
filter pattern:

```rust
// OLD: discards non-admission events
let event = match payload {
    WebhookPayload::PullRequestReview(e) => AdmissionRelevantEvent::PullRequestReview(e),
    WebhookPayload::CheckRun(e) => AdmissionRelevantEvent::CheckRun(e),
    _ => return Ok(state),
};
```

With a match that handles each event type and falls through to admission
processing. The overall structure (after the "is this repo tracked?" check):

```rust
match payload {
    // --- PR lifecycle events ---
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

    // --- Push events (base branch advance) ---
    WebhookPayload::Push(e) => {
        let pushed_ref = &e.ref_;
        for candidate in state.candidates.values_mut() {
            if candidate.repo_url == *clone_url && candidate.base_branch == *pushed_ref {
                candidate.base_sha = e.after.clone();
            }
        }
    }

    // --- Admission events (existing logic) ---
    WebhookPayload::PullRequestReview(e) => {
        let mut events = vec![(e.pull_request.node_id.clone(),
            AdmissionRelevantEvent::PullRequestReview(e))];
        process_admission_events(&mut state, &events, clone_url, api);
    }

    WebhookPayload::CheckRun(e) => {
        let pr_ids = lookup_open_prs_by_sha(api, clone_url, &e.check_run.head_sha);
        let events: Vec<_> = pr_ids.into_iter().map(|id| (id,
            AdmissionRelevantEvent::CheckRun(e.clone()))).collect();
        // Note: CheckRunEvent may not be Clone — if so, wrap in a vec with one element
        // and process directly, or refactor to avoid cloning.
        process_admission_events(&mut state, &events, clone_url, api);
    }

    // --- No-op events ---
    WebhookPayload::Ping(_)
    | WebhookPayload::WorkflowJob(_)
    | WebhookPayload::WorkflowRun(_) => {}
}

Ok(state)
```

Extract the repeated admission processing logic into a helper:

```rust
fn process_admission_events(
    state: &mut CqActorState,
    events: &[(String, AdmissionRelevantEvent)],
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
```

Note: verify exact field names on `PullRequestEventDetails` (the `Synchronize`
variant has `before` and `after` fields) and `CandidatePr` against the actual
types.

### Important

`handle_webhook()` no longer has early returns for non-admission events (the
function now returns `Ok(state)` once at the end). This means the actor loop
(Step 10) will always run evaluate→step after any webhook, which is the desired
behavior — webhooks trigger immediate queue cycles.

### Acceptance

- `cargo build --bin josh-cq` succeeds
- `cargo fmt` passes
- All 7 webhook event types are handled (Ping, Push, PullRequest, WorkflowJob,
  WorkflowRun, CheckRun, PullRequestReview) — only Ping, WorkflowJob, WorkflowRun
  are no-ops
- The function returns `Ok(state)` at a single exit point
