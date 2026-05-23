# Step 2: Add GraphQL query to list open PRs

## Why

The fetch step needs to discover all open PRs for a tracked repository. Currently
only `find_open_prs_by_head_sha` exists (lookup PRs by a known SHA). We need
the reverse: list all open PRs to seed the candidate pool.

## What to change

### File: `forges/josh-github-codegen-graphql/src/get_open_prs.graphql` (new)

Add a GraphQL query that lists open PRs for a repository with pagination:

```graphql
query GetOpenPrs($owner: String!, $name: String!, $first: Int!, $after: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(first: $first, after: $after, states: [OPEN]) {
      nodes {
        id
        number
        title
        headRefOid
        headRefName
        baseRefOid
        baseRefName
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
```

Check the schema at `forges/josh-github-codegen-graphql/src/github.graphql` for the
exact field names. The fields above are the GitHub GraphQL API v4 names; adjust if
the introspection schema uses different casing (e.g., `headRefOid` vs `headRefOid`).

### File: `forges/josh-github-codegen-graphql/src/manifest.toml`

Add a section:

```toml
[queries.get_open_prs]
```

No fragments needed.

### File: `forges/josh-github-graphql/src/operations/pull_request.rs` (or new file)

Implement a method on `GithubApiConnection`:

```rust
/// Represents an open pull request discovered during fetch.
#[derive(Debug, Clone)]
pub struct OpenPr {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub head_sha: String,
    pub head_branch: String,
    pub base_sha: String,
    pub base_branch: String,
}

impl GithubApiConnection {
    pub async fn get_open_pull_requests(
        &self,
        owner: &str,
        name: &str,
    ) -> anyhow::Result<Vec<OpenPr>> {
        // Paginate through all open PRs using the generated query type.
        // Use first: 100, loop while pageInfo.hasNextPage.
    }
}
```

Define `OpenPr` in `josh-github-graphql/src/operations/pull_request.rs` (or in
`josh-cq`'s own types, then convert — the agent should decide which is cleaner).

Follow the pagination pattern from `get_maintainers()` in
`forges/josh-github-graphql/src/operations/collaborators.rs`.

### Acceptance

- `cargo build` succeeds (the codegen crate builds the new query, the graphql
  crate compiles the wrapper)
- No warnings about unused items (the new function will be used in Step 7)
- `cargo fmt` passes
