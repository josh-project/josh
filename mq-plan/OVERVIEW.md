# Merge Queue Implementation Plan

## Architecture

The merge queue runs inside `josh-cq serve` using an **actor model**. All input
— webhook events, track requests, and periodic polling ticks — is sent through
a single mpsc channel. A single `spawn_blocking` task processes events serially,
mutating `CqActorState` without locks or concurrent access. After each event,
the actor runs evaluate→step to merge any admissible PRs.

This design avoids concurrency bugs: state mutations are serialized, and the
queue cycle never overlaps with webhook handling.

### Event flow

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

- **Webhooks** update admission state immediately and trigger the queue cycle.
- **Tick** (every 10 min) runs a full fetch + evaluate + step as a fallback for
  missed webhooks or state drift.

### Merge flow (step)

1. Select first admissible PR (maintainer approved, checks pass, no changes requested).
2. Compute merge locally in the metarepo (`git merge-tree` + `git commit-tree`).
3. Push the merge commit to the remote's main branch.
4. Update `.link.josh` in the metarepo to point to the merge commit.
5. Close the PR on GitHub and post a "merged by Josh MQ" comment.
6. Remove the PR from the candidate pool. Repeat while more PRs are admissible.

No speculation, no batching, no dependency ordering. One PR at a time.

## Design decisions

- **Merge happens in the metarepo**, not via GitHub's merge API. We create a
  merge commit locally, push it to the remote's main, then close the PR and
  post a "merged by MQ" comment.
- **git CLI for plumbing**, consistent with existing patterns (`spawn_git_command`
  used in `handle_track`). We use `git merge-tree` + `git commit-tree` + `git push`.
- **Actor model** — all input goes through an mpsc channel, processed serially
  by a single `spawn_blocking` task. No locks, no concurrent state access.
- **Hybrid trigger** — webhooks trigger immediate queue cycles; a 10-minute
  polling tick catches anything missed (e.g., webhook delivery failures).
- **Only `serve` remains** — the `fetch`/`step`/`push` CLI subcommands are
  removed. Only `init`, `track`, and `serve` remain.

## Key crates involved

| Crate | Role |
|---|---|
| `josh-cq` | Merge queue logic, CLI, serve loop |
| `josh-github-graphql` (`forges/josh-github-graphql`) | GraphQL client, new queries |
| `josh-github-codegen-graphql` (`forges/josh-github-codegen-graphql`) | Generated GraphQL code (add .graphql files + manifest entries) |
| `josh-github-changes` (`forges/josh-github-changes`) | `AdmissionState`, `parse_owner_repo` |
| `josh-github-webhooks` (`forges/josh-github-webhooks`) | Webhook type definitions |
| `josh-link` | `update_links()`, `prepare_link_add()` |
| `josh-core` | `find_link_files`, `spawn_git_command`, filters |

## Steps

1. [Remove `fetch`/`step`/`push` CLI subcommands](STEP_1.md)
2. [Add GraphQL query to list open PRs](STEP_2.md)
3. [Add GraphQL query to fetch PR reviews](STEP_3.md)
4. [Add GraphQL mutation to post PR comment](STEP_4.md)
5. [Add `CandidatePr` model and extend `CqActorState`](STEP_5.md)
6. [Handle `PullRequest` and `Push` webhook events](STEP_6.md)
7. [Implement `handle_fetch()`](STEP_7.md)
8. [Implement candidate evaluation](STEP_8.md)
9. [Implement `handle_step()`](STEP_9.md)
10. [Wire actor loop with tick timer into `serve`](STEP_10.md)
