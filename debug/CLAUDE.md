# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & check

```bash
cargo build                          # debug build
cargo build --release                # release build
cargo clippy                         # lint
```

There are no tests yet.

## Architecture

Rust desktop GUI app that visualizes git repository trees. Built with `egui`/`eframe` (immediate-mode GUI) and `git2` (libgit2 bindings). Cargo workspace with two crates.

### Workspace structure

```
├── Cargo.toml            # workspace root
├── git-tree-viewer/      # GUI app (binary + library)
│   ├── Cargo.toml
│   └── src/
└── git-tree-trace/       # trace sender library
    ├── Cargo.toml
    └── src/
```

### Entry point

`git-tree-viewer/src/bin/git-tree-viewer.rs` — parses CLI args via `clap` (derive), then calls `show_repo_viewer` from the library.

CLI flags:
- `--rev-spec <spec>` (optional) — commit to view (SHA, branch, tag, `HEAD~3`). Only used in Browse mode.
- `--mode <browse|trace>` (optional) — app mode. If omitted, shows a small GUI dialog with two buttons to select.

When mode is omitted, `select_mode()` opens a small `eframe` window (320×150) with "Browse" and "Trace" buttons. The chosen mode is sent through a `mpsc::channel` back to `main()`.

### Modes

The app has two modes (`AppMode` enum in `lib.rs`):

- **Browse** — opens the current directory as a git repo and shows commit history, tree, and file preview. The `rev_spec` field holds the optional `--rev-spec` CLI arg.
- **Trace** — creates a temporary bare repo via `RepoSource::new_temp()`, starts an HTTP server on `127.0.0.1:8765`, and shows a session filter dropdown, trace list, commit history, tree, and file preview. The `traces` vector and `rx` channel receiver are stored inside the enum variant.

### Module structure (git-tree-viewer)

```
git-tree-viewer/src/
├── lib.rs              # GitDebugApp, UiState, Trace, AppMode, RepoSource, show_repo_viewer
├── app.rs              # eframe::App impl — polls trace channel, delegates to ui::panels
├── constants.rs        # Layout/font/sizing constants
├── server.rs           # HTTP server (axum on 127.0.0.1:8765) — trace endpoint + git HTTP
├── git/
│   ├── mod.rs          # Re-exports
│   ├── repo.rs         # open_repo + resolve_commit
│   ├── tree.rs         # TreeItem enum + build_tree (recursive tree traversal)
│   └── blob.rs         # Blob content loading (UTF-8 vs binary detection)
├── ui/
│   ├── mod.rs          # Re-exports
│   ├── panels.rs       # Top-level panel layout assembly
│   ├── commit_list.rs  # show_commit_bubble + show_commits
│   ├── tree_view.rs    # tree_entry_label + show_tree_item (recursive)
│   └── file_preview.rs # File content display
└── bin/
    └── git-tree-viewer.rs
```

### Key types (lib.rs)

```rust
pub enum AppMode {
    Browse { rev_spec: Option<String> },
    Trace { traces: Vec<Trace>, rx: std::sync::mpsc::Receiver<Trace> },
}

pub enum RepoSource {
    TempDir(tempfile::TempDir),
    Path(PathBuf),
}

pub struct Trace {
    pub session: String,
    pub commit: String,
    pub label: String,
}

pub struct UiState {
    pub history_start: Option<git2::Oid>,
    pub selected_commit: Option<git2::Oid>,
    pub selected_file: Option<(String, git2::Oid)>,
    pub file_content: Option<String>,
    pub selected_session: Option<String>,
    pub error: Option<String>,
}

pub struct GitDebugApp {
    mode: AppMode,
    repo: git2::Repository,
    ui_state: UiState,
}
```

- `RepoSource::new_temp()` creates a bare git repo in a temp directory with `http.receivepack=true` set.
- `RepoSource` implements `AsRef<Path>`.
- `GitDebugApp::new(mode, repo_source)` opens the repo via `git::open_repo()`. In Browse mode, resolves the `rev_spec` commit and sets both `history_start` and `selected_commit` to it. In Trace mode, both start as `None`.

### Layers

**`git/`** — pure git operations. Functions take `&Repository` and OIDs, return data. No egui dependency, no state mutation.

- `open_repo(path)` — calls `Repository::discover(path)`.
- `resolve_commit(repo, spec: Option<&str>)` — if `spec` is `Some`, calls `revparse_single` + `peel_to_commit`; if `None`, resolves `HEAD`.
- `build_tree(repo, tree_oid, path_prefix)` — recursively traverses a `git2::Tree`. The `path_prefix` parameter accumulates the path during recursion (empty string for the root call).
- `load_blob_content(repo, oid)` — returns UTF-8 text or `"<Binary file>"` for non-UTF-8 blobs.

**`ui/`** — rendering functions. Each panel/sub-panel is a standalone function that takes only the state it needs. `show_commit_bubble` is re-exported from both `ui::commit_list` and `lib.rs`. `show_commits` takes `history_start: Option<Oid>` and mutable references to `selected_commit`, `selected_file`, and `file_content`.

**`app.rs`** — thin `eframe::App` impl. On each frame, drains any pending traces from the `mpsc` channel (only in Trace mode), then delegates to `ui::panels::show_panels`.

**`lib.rs`** — `GitDebugApp` struct definition + `new()` constructor + `show_repo_viewer` entry point.

**`server.rs`** — axum HTTP server running in a separate thread. Listens on `127.0.0.1:8765`, serves:
- `POST /v1/traces` — accepts JSON `{session, commit, label}`, appends to the server's trace store and sends a `Trace` through the `mpsc::Sender`.
- `GET /v1/traces` — returns the full list of received traces as a JSON array of `{session, commit, label}`.
- `GET /v1/repo` — returns the temp repo location as `{ "path": "/tmp/..." }`.
- Everything else (fallback) — serves git HTTP via `josh_cq_test_components::git_http::serve()`, enabling `git push` to the temp repo.

The `start()` function takes both a `Sender<Trace>` and a `repo_path: &Path`.

Server state is `ServerState { tx: Arc<Sender<Trace>>, traces: Arc<Mutex<Vec<Trace>>>, repo_path: Arc<Path> }`.

### GitDebugApp construction

`GitDebugApp::new(mode, repo_source)` opens the repo and resolves the initial commit:

- **Browse mode**: resolves `rev_spec` (or HEAD) via `git::resolve_commit()`, sets both `history_start` and `selected_commit` to the resolved OID.
- **Trace mode**: both `history_start` and `selected_commit` start as `None`. Commits appear as traces arrive.

### GUI layout

| Panel | Content |
|---|---|
| `Panel::top` | Title bar, error display |
| `Panel::left` (300px) | **Trace mode:** session dropdown + trace list on top, commits on bottom (resizable split). **Browse mode:** commits only. |
| `Panel::right` (300px) | File preview — monospaced text, or `<Binary file>` for non-UTF-8 |
| `CentralPanel` | Recursive directory tree built from the selected commit's tree OID |

### Tree building

`build_tree(repo, tree_oid, path_prefix)` recursively traverses a `git2::Tree`. Entries are sorted: directories before files, then alphabetically within each group. Returns `Vec<TreeItem>` where `TreeItem` is an enum with named fields:

```rust
pub enum TreeItem {
    Directory { name: String, oid: Oid, children: Vec<TreeItem> },
    File { name: String, full_path: String, oid: Oid },
    Other { name: String, oid: Oid },
}
```

`path_prefix` accumulates the directory path during recursion (empty string `""` for the initial call). `full_path` is `"{path_prefix}/{name}"` for nested files.

### Commit resolution

`git::resolve_commit()` accepts full/short SHA-1s, branch names, tag names, and relative refs (`HEAD~3`). If `spec` is `None`, resolves `HEAD`.

### git-tree-trace crate

Library crate that provides `trace_commit(repo: &git2::Repository, oid: Oid, name: &str)`. This is now an `async` function. It does two things:

1. Runs `git push http://127.0.0.1:8765 {oid}:refs/heads/_{oid}` via `tokio::process::Command` to push the commit to the viewer's temp repo.
2. When running in a test environment (detected via `NEXTEST` env var or `deps/` in the executable path), sends an HTTP POST to `http://127.0.0.1:8765/v1/traces` with `{session, commit, label}`. The session name is `trace-{unix_timestamp}`. Outside test environments, the HTTP trace is skipped (but the git push still happens).

Uses `OnceLock` for global state (`TraceState` enum, `reqwest::Client`).

### Key dependencies

| Crate | Role |
|---|---|
| `git2` (0.21) | git-tree-viewer: all git operations — repo discovery, rev parsing, tree/commit/blob traversal. Features: `vendored-openssl`, `https` |
| `git2` (0.20) | git-tree-trace: OID type (minimal, no default features) |
| `egui`/`eframe` (0.34.2) | Immediate-mode GUI, wgpu backend |
| `clap` (4.6.1) | CLI argument parsing (derive mode) |
| `anyhow` (1) | Error handling in public API (features: `backtrace`) |
| `axum` (0.8) | HTTP server (minimal: http1, json, tokio only) |
| `tokio` (1) | Async runtime. git-tree-viewer: `rt-multi-thread`, `net`. git-tree-trace: `process` |
| `reqwest` (0.12) | git-tree-trace: blocking HTTP client (rustls-tls) |
| `tracing` (0.1) | git-tree-trace: error logging for failed trace sends |
| `serde`/`serde_json` (1) | JSON serialization for `/v1/traces` requests and trace sending |
| `josh-cq-test-components` | git-tree-viewer: `git_http::serve()` for the fallback git HTTP handler |
| `tempfile` (3) | git-tree-viewer: temp directory for bare repo in Trace mode |
