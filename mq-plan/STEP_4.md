# Step 4: Add GraphQL mutation to post PR comment

## Why

When the merge queue merges a PR, it closes the PR on GitHub and posts a comment
explaining that the PR was merged by the external merge queue. The `close_pull_request`
mutation already exists; this step adds the comment mutation.

If the GitHub GraphQL API doesn't support adding PR comments directly (it uses
`AddComment` on issues, and PRs have an associated issue), check the schema. Fall
back to the REST API (`POST /repos/{owner}/{repo}/issues/{number}/comments`) if
GraphQL is impractical.

## What to change

### Option A: GraphQL (preferred if schema supports it)

#### File: `forges/josh-github-codegen-graphql/src/add_pr_comment.graphql` (new)

```graphql
mutation AddPrComment($subjectId: ID!, $body: String!) {
  addComment(input: { subjectId: $subjectId, body: $body }) {
    clientMutationId
  }
}
```

Note: `subjectId` is the PR's `node_id`. The `addComment` mutation works on any
`Commentable` node, which includes pull requests.

#### File: `forges/josh-github-codegen-graphql/src/manifest.toml`

Add:

```toml
[queries.add_pr_comment]
```

#### File: `forges/josh-github-graphql/src/operations/pull_request.rs`

```rust
impl GithubApiConnection {
    pub async fn add_pr_comment(
        &self,
        pr_node_id: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        // Call the addComment mutation with subjectId = pr_node_id
    }
}
```

### Option B: REST API fallback

If the GraphQL approach doesn't work, implement via REST:

#### File: `forges/josh-github-graphql/src/operations/pull_request.rs`

```rust
impl GithubApiConnection {
    pub async fn add_pr_comment(
        &self,
        owner: &str,
        name: &str,
        pr_number: i64,
        body: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            crate::request::GITHUB_REST_API_URL,
            owner,
            name,
            pr_number
        );
        // POST with JSON { "body": body }
        // Use self.client to make the request
    }
}
```

### Acceptance

- `cargo build` succeeds
- The new method compiles and is callable
- `cargo fmt` passes
