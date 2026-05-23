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
| `josh-cq` | **Merge queue** (current focus) |
| `josh-github-graphql` | GitHub GraphQL client (`GithubApiConnection`, PR/collaborator/ruleset queries) |
| `josh-github-auth` | GitHub device-flow OAuth, `GithubAuthMiddleware`, token refresh |
| `josh-github-keyring` | Credential storage (macOS Keychain or `~/.config/josh-cli/credentials.json`) |
| `josh-github-changes` | `AdmissionState` (check runs + maintainer reviews → admissible), PR creation/update, repo URL parsing |
| `josh-github-webhooks` | Webhook type definitions, payload deserialization, HMAC signature verification |
| `josh-test-webhook-service` | Webhook relay server: receives GitHub webhooks, broadcasts to WS clients |
| `josh-test-webhook-client` | WS client connecting to relay, forwards webhooks to CQ's `/v1/webhook` |
| `josh-test-github` | Simulated GitHub environment: `SimRepo`, `GraphQLMock`, `GitServer`, `TestRepo` |
| `josh-cq-tests` | Integration tests for the merge queue (end-to-end: metarepo → webhook → merge → close) |

## Merge queue (josh-cq)

### Subcommands

The CQ binary (`josh-cq`) supports:
- **`init`** — creates an empty metarepo commit on HEAD
- **`serve`** — starts HTTP server receiving webhooks and track requests
- **`track`** — adds a remote as a tracked repo (git ls-remote + `.link.josh` + `refs.json`)

### Architecture

The `serve` event loop uses an actor model. All input — webhook events, track
requests, and periodic polling ticks — is sent through a single mpsc channel.
A single `spawn_blocking` task processes events serially, mutating
`CqActorState` without locks.

The queue cycle: **Fetch** → **Evaluate** → **Step** (merge) → repeat while
admissible PRs remain.

- **Tick** (every 10 min) triggers a full fetch→evaluate→step cycle
- **Webhooks** update admission state immediately and trigger the queue cycle
- **Merge** happens in the metarepo via `git merge-tree --write-tree` + `git commit-tree`,
  then pushes to the remote's main branch and closes the PR on GitHub

### State model

`CqActorState` fields:
- `admission`: per-repo required status checks (from GitHub rulesets)
- `pr_admissions`: per-PR `AdmissionState` (reviews, check run results)
- `candidates`: open PRs discovered during fetch
- `url_owner_map`: maps arbitrary clone URLs to (owner, name) pairs (used for non-GitHub URLs in tests)
- `closed_prs`: tracks PRs closed via webhook to prevent re-discovery on next fetch

`AdmissionState` tracks per-PR:
- `required_checks` — required status checks from GitHub rulesets, each paired with a passed/failed bool
- `maintainer_reviews` — per-maintainer review state (Approved / ChangesRequested / etc.)
- `maintainers` — set of users with write access to the repo
- `admissible()` returns true when: ≥1 maintainer approved, no maintainer requested changes, all required checks passed

### API connection

`GithubApiConnection::from_environment()` resolves credentials from `GH_TOKEN` env var or the stored device-flow token (set by `josh auth login github`).
`GithubApiConnection::for_test(url)` creates an unauthenticated client pointing at a mock GraphQL server.

### Integration tests

Integration tests live in `josh-test-cq/tests/merge_queue_tests.rs`. They use a
simulated GitHub environment (`SimRepo` + `GraphQLMock` + git HTTP remote) and
exercise the full merge queue flow:

| Test | Scenario |
|------|----------|
| `merge_single_pr` | PR with approving review and no checks → merged |
| `pr_not_admissible_without_review` | PR with no approving review → not merged |
| `pr_not_admissible_with_failing_check` | PR with failing required check → not merged |
| `pr_removed_on_close_webhook` | PR closed via webhook → not merged |

Run with: `cargo test -p josh-cq-tests --test merge_queue_tests -- --test-threads=1`
