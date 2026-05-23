# Step 14: Add mock GraphQL server

## Why

The merge queue queries GitHub's GraphQL API for PRs, reviews, rulesets,
maintainers, check suites, and to close PRs/post comments. A mock HTTP server
that responds to these specific queries lets integration tests control the
"GitHub view" of PR state without talking to real GitHub.

The mock matches incoming requests by `operationName` in the JSON body and
returns pre-configured responses. It also tracks mutation calls so tests can
assert "close PR was called with the right node ID."

## What to change

### File: `forges/josh-test-github/src/lib.rs`

Add module:
```rust
pub mod graphql_mock;
```

### File: `forges/josh-test-github/Cargo.toml`

No new dependencies needed. The mock uses `axum`, `serde_json`, `tokio` — all
already present.

### File: `forges/josh-test-github/src/graphql_mock.rs` (new)

#### Data model

```rust
use std::collections::BTreeMap;
use std::sync::Mutex;
use url::Url;

pub struct GraphQLMock {
    state: Arc<GraphQLState>,
}

struct GraphQLState {
    // Configurable state (set by tests before running)
    pub prs: Vec<MockPr>,
    pub reviews: BTreeMap<i64, Vec<(String, String)>>, // pr_number → [(login, state)]
    pub maintainers: Vec<String>,
    pub rulesets: Vec<MockRuleset>,
    pub required_checks: Vec<String>, // check context names

    // Mutation tracking (appended by the mock during the test)
    pub closed_prs: Mutex<Vec<String>>,
    pub comments: Mutex<Vec<(String, String)>>, // (subject_id, body)
}

pub struct MockPr {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub head_ref_name: String,
    pub head_ref_oid: String,
    pub base_ref_name: String,
    pub base_ref_oid: String,
}

pub struct MockRuleset {
    pub id: String,
    pub name: String,
    pub enforcement: String, // "ACTIVE" or "DISABLED"
    pub include_refs: Vec<String>,
    pub exclude_refs: Vec<String>,
}
```

#### Operation dispatch

The server receives `{ "query": "...", "variables": {...}, "operationName": "X" }`
and dispatches to handlers based on `operationName`:

| `operationName` | Handler |
|---|---|
| `GetOpenPrs` | Returns `prs` with pagination fields (`pageInfo`, `totalCount`) |
| `GetPrReviews` | Returns reviews for the PR number in `variables.number` |
| `GetPrsBySha` | Returns PRs whose `head_ref_oid` matches `variables.sha` |
| `GetRepositoryCollaborators` | Returns `maintainers` as edges with `WRITE` permission |
| `GetRepositoryRulesets` | Returns `rulesets` list |
| `GetRulesetRequiredChecks` | Returns required status checks with `context` fields |
| `ClosePullRequest` | Records node ID in `closed_prs`, returns success payload |
| `AddPrComment` | Records `(subject_id, body)` in `comments`, returns success payload |

Each handler uses `serde_json::json!({ ... })` to construct the exact response
shape that `josh-github-graphql`'s generated code expects (matching the
`graphql_client::Response<T::ResponseData>` format).

#### API

```rust
impl GraphQLMock {
    pub fn new() -> Self { ... }

    // State configuration (builder pattern)
    pub fn with_pr(mut self, pr: MockPr) -> Self { ... }
    pub fn with_review(mut self, pr_number: i64, login: &str, state: &str) -> Self { ... }
    pub fn with_maintainer(mut self, login: &str) -> Self { ... }
    pub fn with_ruleset(mut self, ruleset: MockRuleset) -> Self { ... }
    pub fn with_required_check(mut self, context: &str) -> Self { ... }

    // Start the server, returns (join_handle, api_url)
    pub async fn serve(self) -> anyhow::Result<(tokio::task::JoinHandle<()>, Url)> { ... }

    // Mutation inspection (after the test)
    pub fn closed_pr_node_ids(&self) -> Vec<String> { ... }
    pub fn comments(&self) -> Vec<(String, String)> { ... }
}
```

#### Important: global IDs

The merge queue uses PR node IDs (GraphQL global IDs) like `"PR_kwDO..."`. The
mock must accept whatever node ID the test provides in `MockPr.node_id` and echo
it back in responses (for `ClosePullRequest`, `AddPrComment`, PR lookups).

When the merge queue calls `api.close_pull_request(&candidate.node_id)`, the
node ID was obtained from a previous `GetOpenPrs` response. The mock must return
the same `node_id` in both responses so they match up.

#### Pagination

`GetOpenPrs` supports cursor-based pagination (`first`, `after`). The mock
should:
- If `variables.first` is larger than the number of PRs, return all PRs with
  `hasNextPage: false`
- If `variables.first` is smaller, return the first page with a cursor and
  `hasNextPage: true`
- `GetPrReviews` and `GetRepositoryCollaborators` follow the same pattern

For MVP tests with 1-2 PRs, a single page is sufficient. Implement minimal
pagination (return all items up to `first`, set `hasNextPage: false`).

### Acceptance

- `cargo build -p josh-test-github` succeeds
- `cargo fmt` passes
