# Step 12: Create `josh-test-github` crate with `TestRepo` and `GitServer`

## Why

Integration tests need a simulated GitHub repository: a bare git repo served over
HTTP that can receive commits and trigger post-receive hooks. This step creates
the foundation crate with two low-level building blocks:

- `TestRepo` — creates and mutates bare git repos in temp directories
- `GitServer` — serves a bare repo over HTTP using `git http-backend`

## What to change

### File: `Cargo.toml` (workspace root)

Add `forges/josh-test-github` to the `members` list.

### File: `forges/josh-test-github/Cargo.toml` (new)

```toml
[package]
name = "josh-test-github"
version = "26.5.8"
edition = "2024"
license-file = "../../LICENSE"

[dependencies]
tokio.workspace = true
axum.workspace = true
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
git2.workspace = true
tempfile.workspace = true
url.workspace = true
reqwest.workspace = true
chrono.workspace = true
tower-http.workspace = true
tower.workspace = true

josh-github-webhooks.workspace = true
```

### File: `forges/josh-test-github/src/lib.rs` (new)

```rust
pub mod git_server;
pub mod test_repo;
```

### File: `forges/josh-test-github/src/test_repo.rs` (new)

Adapted from `mh-testrepo`. Provides `TestRepo`:

```rust
use std::path::{Path, PathBuf};

pub enum HookType {
    PostReceive,
}

pub struct TestRepo {
    dir: tempfile::TempDir,
    repo: git2::Repository,
    current_branch_ref: String,
}

pub const INITIAL_BRANCH_REF: &str = "refs/heads/main";

impl TestRepo {
    /// Create a new bare git repo in a temp directory.
    pub fn new() -> anyhow::Result<Self> {
        let dir = tempfile::Builder::new().prefix("josh-test-github").tempdir()?;
        let repo = git2::Repository::init_bare(dir.path())?;
        repo.set_head(INITIAL_BRANCH_REF)?;
        Ok(Self {
            dir,
            repo,
            current_branch_ref: INITIAL_BRANCH_REF.to_string(),
        })
    }

    /// Create from an existing temp directory (e.g., a cloned repo).
    pub fn from_tempdir(tempdir: tempfile::TempDir) -> anyhow::Result<Self> { ... }

    /// Install a hook script into the repo's hooks directory.
    pub fn install_hook(&mut self, _hook: HookType, contents: &str) -> anyhow::Result<()> { ... }

    /// Commit a file to the current branch. Returns (commit_oid, tree_oid).
    pub fn commit(
        &mut self,
        file_path: impl Into<PathBuf>,
        content: &str,
        message: Option<&str>,
    ) -> anyhow::Result<(git2::Oid, git2::Oid)> { ... }

    /// Switch to a branch, creating it if it doesn't exist.
    /// Returns Some((commit_oid, tree_oid)) if the branch was newly created, None if it already existed.
    pub fn select_create_branch(
        &mut self,
        branch_name: &str,
    ) -> anyhow::Result<Option<(git2::Oid, git2::Oid)>> { ... }

    // Accessors
    pub fn repo(&self) -> &git2::Repository { &self.repo }
    pub fn path(&self) -> PathBuf { self.dir.path().to_owned() }
    pub fn current_head(&self) -> anyhow::Result<git2::Oid> { ... }
    pub fn current_branch_ref(&self) -> String { self.current_branch_ref.clone() }
}

impl AsRef<Path> for TestRepo {
    fn as_ref(&self) -> &Path { self.dir.path() }
}
```

Key implementation notes:
- Use a `git2::Signature` with `time: 0` for deterministic commit hashes
- `commit()` creates a blob, creates a tree from the blob + any parent tree,
  creates a commit, and updates the branch reference
- `select_create_branch()` checks if the branch exists via `revparse_single`;
  if not, creates it pointing at the current branch's HEAD
- `install_hook()` writes the script to `hooks/post-receive` and makes it executable
  (mode `0o755`)

### File: `forges/josh-test-github/src/git_server.rs` (new)

Adapted from `mh-testserver`. Provides `GitServer`:

```rust
use url::Url;

pub struct GitServer {
    task: tokio::task::JoinHandle<()>,
    port: u16,
}

impl GitServer {
    /// Serve a bare git repo over HTTP on a random local port.
    pub async fn new(repo_path: &std::path::Path) -> anyhow::Result<Self> { ... }

    /// Returns the clone URL: `http://127.0.0.1:{port}/`
    pub fn url(&self) -> Url { ... }
}

impl Drop for GitServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
```

The server uses Axum with two routes:
- `GET /{*path}` — serves git fetch/clone via `git http-backend`
- `POST /{*path}` — serves git push via `git http-backend`

Implementation: spawn `git http-backend` as a child process, pipe request body to
stdin, collect stdout (parsing CGI headers) + stderr, return the response. Use
standard CGI environment variables: `GIT_PROJECT_ROOT`, `PATH_INFO`,
`QUERY_STRING`, `REQUEST_METHOD`, `CONTENT_TYPE`, `CONTENT_LENGTH`.

### Acceptance

- `cargo build -p josh-test-github` succeeds
- `cargo fmt` passes
