# josh run

`josh run` is an experimental `josh` CLI subcommand that runs workspaces in isolated,
automatically-cached containers. It is used to build the josh binaries and run the integration
test suite.

> **Note:** `josh run` is experimental and requires `JOSH_EXPERIMENTAL_FEATURES=1` to be set
> in the environment.

## Motivation

A common problem with monorepo builds is that the container context contains the entire repository — thousands of files most builds don't need. This has two consequences:

1. **Fragile caching.** Docker and podman derive cache tags from the context contents. Any file touched anywhere in the repo can break the cache even if the build doesn't use that file.
2. **Hidden dependencies.** If a build silently reads a file that "happens to be there", it works locally but may break in a clean environment or for another team member.

`josh run` addresses both problems by filtering the repository to exactly the files a given workspace needs before any container is involved:

- The filtered tree's git SHA becomes the cache key. Only changes to files actually included in the workspace invalidate the cache.
- The container sees only what the workspace definition explicitly includes — files outside the workspace are structurally impossible to access.
- The host working tree is never modified. Artifacts produced by the container are stored in a podman volume and exported back when the run completes.

## Prerequisites

- **podman** installed and on `$PATH`
- **josh** installed and on `$PATH`:
- `JOSH_EXPERIMENTAL_FEATURES=1` set in your environment

  ```sh
  cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
  ```

## Quick start

All commands are run from the root of the josh repository.

### Run all tests

```sh
josh run
```

With no arguments, `josh run` looks for a `run.josh` file in the repository root and uses it as the filter. In this repo, `run.josh` points to `ws/test.josh`, so `josh run` runs the full test suite.

### Run a specific workspace

```sh
josh run . :+ws/build-rust
```

### Use the staged index instead of the working tree

```sh
josh run + :+ws/test
```

### Build from the last commit (ignoring local changes)

```sh
josh run HEAD :+ws/test
```

## Syntax

```
josh run [OPTIONS] [REFERENCE] [FILTER]
```

| Argument | Description |
|---|---|
| `[REFERENCE]` | Git ref to build from. Defaults to `.` (working tree). |
| `[FILTER]` | Josh filter selecting the workspace to run. Defaults to `:+run` (reads `run.josh`). |

### `[REFERENCE]` values

| Value | Meaning |
|---|---|
| `.` (default) | Working tree, including uncommitted changes |
| `+` | Staged files only (git index). Useful to test exactly what you have `git add`ed. |
| `HEAD` | Last commit, ignoring any local changes. Useful for clean builds or before/after comparisons. |
| Any git ref or SHA | Build from that specific commit. |

### Options

| Flag | Description |
|---|---|
| `--clean` | Remove cached images and output volumes |
| `--clean-all` | Remove cached images, output volumes, and persistent cache volumes |

## Inspecting test results

Near the start of the output, `josh run` prints the `WS_TREE` SHA:

```
WS_TREE: abc123def456...
```

The prysk-updated `.t` test files (showing diffs for any failures) are stored in the podman volume `out_<WS_TREE>` under `tests/`, not in the working directory.

```sh
# List all test result files
podman volume export out_<WS_TREE> | tar -tvf - tests/

# Print a specific test file to stdout
podman volume export out_<WS_TREE> | tar -xOf - tests/filter/foo.t
```

For failing tests the prysk diff format shows: the shell command that failed, the expected output (indented two spaces), and the actual output (preceded by `+`).

The final lines of output report the overall result:

```
# Ran N tests, M skipped, K failed.
SUCCESS: <safe-name>
```

or

```
FAILED: <safe-name>
```

## Cache behavior

Each run produces a podman volume named `out_<WS_TREE>`. Successful runs are also recorded under `.josh/success/<WS_TREE>`. A cached result is reused only when that success marker is present and, for workspaces that keep output, the matching `out_<WS_TREE>` volume still exists. The cache key is the git SHA of the filtered workspace tree, so:

- Changing any file included in the workspace automatically produces a new SHA and bypasses the cache.
- Changing unrelated files has no effect on the cache.
- Two developers with identical workspace contents share the same cache key (useful if volumes are shared via a registry).

### Forcing a re-run

To re-run without changing source files, remove the output volume manually:

```sh
# Find the relevant volume
podman volume ls | grep out_

# Remove it
podman volume rm out_<sha>
```

Alternatively, use `josh run --clean` to remove all cached images and output volumes, or `--clean-all` to also remove persistent cache volumes (e.g. the Cargo registry cache).

### Clearing all output volumes

```sh
podman volume ls -q | grep '^out_' | xargs podman volume rm
```

## Workspace definitions

A workspace is defined by a `.josh` file, typically under `ws/`. The file uses josh filter expressions to declare what the workspace needs and how to run it.

### Workspace keys

| Key | Purpose |
|---|---|
| `:#image[:+path/to/image]` | Container image workspace to build from |
| `:$label="..."` | Human-readable label shown in output |
| `:$cmd="..."` | Command to run inside the container |
| `:$cache="name"` | Persistent podman volume mounted at `/opt/cache` (e.g. for Cargo's registry) |
| `:$output="none"` | Disable output volume (run produces no extracted artifacts) |
| `:$network="host"` | Container network mode |
| `worktree = :[...]` | Files placed in the container's working directory |
| `inputs = :[...]` | Dependency workspaces; each named entry is run first and its output is mounted inside the container |
| `env = :[...]` | Environment variables injected into the container |

### Example: `ws/fetch.josh`

```
:$label="cargo fetch"
:#image[:+images/dev-local]
:$cache="rust"
:$network="host"

:$cmd="cargo fetch --locked"

worktree = :[
    ::**/Cargo.toml
    ::**/Cargo.lock
    ::**/rust-toolchain.toml
    ::**/lib.rs
    ::**/main.rs
]
```

This workspace fetches Cargo dependencies into a persistent cache volume. Only the files needed to resolve the dependency graph are included, so the cache is invalidated only when those files change.

### Example: `ws/build-rust.josh`

```
:$label="rust build"
:#image[:+images/dev-local]
:$cache="rust"

inputs = :[
    :#fetch[:+ws/fetch]
]

env = :[
    ::JOSH_VERSION=VERSION_STRING
]

worktree = :[
    ::run.sh=ws/build-rust.sh
    ::Cargo.toml
    ::Cargo.lock
    ::rust-toolchain.toml
    ::josh-*/
    ::forges/
]
```

This workspace:
- Declares `ws/fetch` as an input dependency; its output (the populated Cargo cache) is mounted before the build runs.
- Injects the `JOSH_VERSION` environment variable from the `VERSION_STRING` file.
- Places `ws/build-rust.sh` into the container as `run.sh` (the entrypoint).
- Includes only the source trees needed to compile.

## Creating a new workspace

1. **Write a `.josh` file** under `ws/`. Declare at minimum an `:#image[...]` reference and a `worktree` subtree containing the files your build needs.

2. **Write the entrypoint** either as a `run.sh` in the worktree or via `:$cmd="..."`. Place any outputs you want extracted under `/out` inside the container (unless `:$output="none"`).

3. **Run it:**

   ```sh
   josh run . :+ws/my-workspace
   ```

4. **Add dependencies** via `inputs = :[...]` if your workspace needs the output of another workspace. Each named entry in `inputs` is run first and its output volume is mounted at `/<name>` inside the container.
