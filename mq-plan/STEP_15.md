# Step 15: Add `GithubApiConnection` constructors and wire API override

## Why

`GithubApiConnection` currently constructs itself from environment variables
(`GH_TOKEN` or stored device-flow token). Integration tests need to inject a
connection pointed at the mock GraphQL server, without real credentials.

Additionally, `spawn_serve_task` hardcodes the API connection creation. It needs
to accept an optional override so tests can supply a mock-backed connection.

## What to change

### File: `forges/josh-github-graphql/src/connection.rs`

Add two constructors:

```rust
impl GithubApiConnection {
    /// Construct with an explicit client and API URL (for testing).
    pub fn new(
        client: reqwest_middleware::ClientWithMiddleware,
        api_url: Url,
    ) -> Self {
        Self { client, api_url }
    }

    /// Construct without authentication, pointed at a custom URL (for testing).
    pub fn for_test(api_url: Url) -> Self {
        Self::new(
            reqwest_middleware::ClientBuilder::new(reqwest::Client::new()).build(),
            api_url,
        )
    }
}
```

The `client` field and `api_url` field are already `pub`, but adding explicit
constructors is cleaner and avoids requiring callers to know about
`reqwest_middleware::ClientBuilder`.

### File: `josh-cq/src/server.rs`

Change `spawn_serve_task` to accept an optional `api` parameter:

```rust
pub fn spawn_serve_task(
    repo_path: PathBuf,
    cache: Arc<CacheStack>,
    tick_interval_secs: u64,
    api: Option<Arc<GithubApiConnection>>,
) -> mpsc::Sender<CqEvent>
```

Inside the function, replace:

```rust
let api: Option<Arc<GithubApiConnection>> =
    GithubApiConnection::from_environment().map(Arc::new);
```

with:

```rust
let api: Option<Arc<GithubApiConnection>> = api.or_else(|| {
    GithubApiConnection::from_environment()
}).map(Arc::new);
```

### File: `josh-cq/src/bin/josh-cq.rs`

Update the call site of `spawn_serve_task` to pass `None` for the new parameter:

```rust
let _event_tx = josh_cq::server::spawn_serve_task(
    repo_path,
    cache,
    args.tick_interval_secs,
    None, // use environment credentials (production path)
);
```

### Acceptance

- `cargo build -p josh-cq` succeeds
- `cargo build -p josh-github-graphql` succeeds
- `cargo fmt` passes
- `cargo clippy -p josh-cq` passes with no new warnings
