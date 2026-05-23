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

## Merge queue (josh-cq)

### Current state

The CQ binary (`josh-cq`) supports these subcommands:
- **`init`** — creates an empty metarepo commit on HEAD
- **`serve`** — starts HTTP server receiving webhooks and track requests
- **`track`** — adds a remote as a tracked repo (git ls-remote + `.link.josh` + `refs.json`)
- **`fetch` / `step` / `push`** — stubbed (`todo!()`)

The `serve` event loop (`cq.rs:spawn_serve_task`) receives `CqEvent`s via an mpsc channel:
- `Track` → calls `handle_track()` which creates the `.link.josh` file for a remote
- `Webhook` → calls `handle_webhook()` which updates `AdmissionState` for relevant PRs

`AdmissionState` tracks per-PR:
- Required status checks (from GitHub rulesets) and whether each passed
- Maintainer reviews (who approved/requested changes/dismissed)
- `admissible()` returns true when: ≥1 maintainer approved, no maintainer requested changes, all required checks passed

`GithubApiConnection::from_environment()` resolves credentials from `GH_TOKEN` env var or the stored device-flow token (set by `josh auth login github`). Local testing flow: `josh auth login github` → `cargo run --bin josh-cq -- serve`.

### Planned implementation

The simplest possible merge queue, without stacking. One PR at a time:

1. **Fetch** — pull latest from tracked remotes, update `.link.josh` files, discover open PRs
2. **Evaluate** — check `admissible()` for each candidate PR; pick the first eligible one
3. **Step** — rebase-merge the selected PR into the metarepo
4. **Push** — push updated metarepo state back to remotes

No speculation, no batching, no dependency ordering between PRs. Just sequential: find an admissible PR, merge it, repeat.
