# Step 3: Add GraphQL query to fetch PR reviews

## Why

During fetch, we need to seed `AdmissionState` with the current review state for
each open PR. Without this, the queue would only know about reviews that arrived
via webhooks after `serve` started. Querying reviews on fetch gives us the full
picture from the start.

## What to change

### File: `forges/josh-github-codegen-graphql/src/get_pr_reviews.graphql` (new)

```graphql
query GetPrReviews($owner: String!, $name: String!, $number: Int!, $first: Int!, $after: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviews(first: $first, after: $after) {
        nodes {
          author {
            login
          }
          state
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
```

### File: `forges/josh-github-codegen-graphql/src/manifest.toml`

Add:

```toml
[queries.get_pr_reviews]
```

### File: `forges/josh-github-graphql/src/operations/pull_request.rs`

Implement:

```rust
impl GithubApiConnection {
    /// Returns the latest review state for each reviewer on a PR.
    /// Returns Vec<(reviewer_login, review_state)>.
    pub async fn get_pr_reviews(
        &self,
        owner: &str,
        name: &str,
        pr_number: i64,
    ) -> anyhow::Result<Vec<(String, josh_github_webhooks::webhook_types::PullRequestReviewState)>> {
        // Paginate through reviews.
        // Only keep the most recent review per author (later reviews supersede earlier ones).
    }
}
```

The return type uses `PullRequestReviewState` from `josh-github-webhooks`
(`Approved`, `ChangesRequested`, `Commented`, `Dismissed`). Make sure this
crate already depends on `josh-github-webhooks` (check `Cargo.toml`); if
not, add the dependency.

Note: GitHub's GraphQL API returns review states as uppercase strings
(`APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`). Map these to
the `PullRequestReviewState` enum variants.

### Acceptance

- `cargo build` succeeds
- The new method compiles and is callable
- `cargo fmt` passes
