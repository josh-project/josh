# josh compose

`josh compose` is an experimental workspace builder and runner. It filters a repository to the
files declared by a workspace, prepares an isolated container environment, and executes workspace
graphs with content-addressed caching.

> **Note:** `josh compose` requires `JOSH_EXPERIMENTAL_FEATURES=1`.

## Motivation

A common problem with monorepo builds is that the container context contains the entire repository
even when a build needs only a small subset. This has two consequences:

1. **Fragile caching.** Unrelated repository changes invalidate broad container contexts.
2. **Hidden dependencies.** Builds can accidentally read undeclared files that happen to exist
   locally.

`josh compose` filters the repository before starting a container:

- The filtered tree's Git SHA becomes the result-cache key.
- The container sees only files explicitly included by the workspace.
- Container outputs are stored in runtime artifacts and optionally extracted into the working tree.

## Prerequisites

- **podman** or **docker** installed and on `$PATH`
- **josh** installed and on `$PATH`
- `JOSH_EXPERIMENTAL_FEATURES=1` set in the environment

  ```sh
  cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
  ```

## Command model

Configured commands execute once and then reuse successful results while their output artifacts
remain available. Command overrides and shells always execute the selected workspace while
dependencies continue to use their configured results.

| Command | Selected workspace | Dependencies | Result |
|---|---|---|---|
| `run` | Runs the configured command or reuses its result | Run configured commands or reuse their results | Publishes configured results and outputs |
| `run -- COMMAND...` | Always executes the override command | Run configured commands or reuse their results | Ephemeral; does not replace the configured result |
| `shell` | Always starts an interactive command | Run configured commands or reuse their results | Ephemeral; does not replace the configured result |

Images and persistent `josh_cache_*` volumes remain cached in every mode.

## Quick start

All commands are run from the repository root.

### Run the default workspace

```sh
josh compose run
```

With no positional arguments, compose uses the working tree and the `:+compose` filter. In this
repository, `compose.josh` selects `ws/test.josh`, so this command builds the binaries and runs the
integration test graph while reusing successful cached jobs.

### Run a specific workspace

```sh
josh compose run . :+ws/build-rust
```

### Execute a side-effecting command

Pass the command after `--`:

```sh
josh compose run . :+ws/action -- ./perform-action
```

The override always executes while the workspace's dependencies retain their cached results. It gets
the configured mounts, environment, persistent cache, network, and sidecars.

### Override the configured command

```sh
josh compose run . :+ws/build-rust -- objdump -h /build/josh
```

The override gets the same worktree, dependency mounts, environment, persistent cache, network, and
sidecars as the workspace. Its output and exit status are not stored as the configured workspace
result.

### Enter an interactive shell

```sh
josh compose shell . :+ws/build-rust
```

The default command is `/bin/sh`. A command after `--` replaces it. The shell gets a fresh scratch
`/out` mount, so interactive changes cannot mutate the workspace's content-addressed output.

### Select a different Git input

```sh
# Staged index
josh compose run + :+ws/test

# Last commit, ignoring local changes
josh compose run HEAD :+ws/test
```

### Inspect the execution plan

```sh
josh compose graph HEAD :+ws/test
```

`josh compose graph` prints D2 source for the complete workspace and image graph without executing
it.

## Syntax

```text
josh compose run [OPTIONS] [REFERENCE] [FILTER] [-- COMMAND...]
josh compose shell [OPTIONS] [REFERENCE] [FILTER] [-- COMMAND...]
josh compose clean [--all] [--backend BACKEND]
```

| Argument | Description |
|---|---|
| `[REFERENCE]` | Git input. Defaults to `.` (working tree); `+` selects the index. |
| `[FILTER]` | Filter selecting the workspace. Defaults to `:+compose`. |
| `COMMAND...` | Command argv executed instead of the configured command. |

### Options

| Flag | Commands | Description |
|---|---|---|
| `--backend BACKEND` | `run`, `shell`, `clean` | Select `podman` or `docker`. |
| `--arg NAME=VALUE` | `run`, `shell`, `graph`, `list-images`, `list-jobs` | Bind a named revision argument. Repeatable. |
| `--all` | `clean` | Also remove persistent cache volumes. |

### Revision object expressions

Compose filters can select the input commit, a named `--arg` binding, or a fallback when an
argument is absent:

```sh
# Use the input commit's first parent
josh compose run . :+ws/size-delta/s32k148-gcc

# Use an explicit, possibly unrelated baseline
josh compose run --arg baseline=origin/main . :+ws/size-delta/s32k148-gcc
```

The workspace can make that selection in an object-valued filter position:

```josh
:$.={#baseline|#^}
```

| Expression | Value |
|---|---|
| `{#}` | Tree of the selected compose input |
| `{@}` | Selected compose input commit |
| `{#^}` | Tree of the input's first parent |
| `{@~2}` | Input commit after two first-parent steps |
| `{#baseline}` | Tree of the commit bound as `baseline` |
| `{@target^2}` | Second parent commit of the `target` binding |
| `{#baseline\|#^}` | Baseline tree, or the input's first-parent tree when `baseline` is absent |

`#` selects a commit's tree; `@` selects the commit itself. `^`, `^N`, `~`,
and `~N` follow Git's parent and first-parent conventions and can be combined.
Both fallback arms must use the same selector. The primary arm must be a named
binding, so an input-relative fallback such as `{#^\|#baseline}` is rejected.

Each `--arg NAME=VALUE` is resolved and peeled to a commit before the filter
is parsed; currently `VALUE` must be a Git single-revision expression. Names
are ordinary bindings; `input` is reserved, and duplicates are rejected.
Missing, malformed, ranged, ambiguous, and non-commit revision values fail
command setup even when the filter does not reference that argument. A
fallback means only that the named argument was not supplied: an invalid
supplied argument or a missing selected parent is an error.

Object expressions are not text or environment-variable substitution and
never evaluate shell input. Repository ref names enter committed filters only
through `--arg`; each revision argument may use Git's full single-revision
syntax.

Resolution happens once, before compose filtering. The parser lowers every
expression to the existing concrete object-ID operations, so the canonical
filter ID and workspace cache key contain the selected OID. Pretty-printing
therefore prints a hexadecimal OID rather than the original expression, and a
ref moving after plan construction cannot alter that plan.

Quoted commit-message templates are separate syntax and remain dynamic:

```josh
:"commit {@}, tree {#}"
```

Here `{@}` and `{#}` are template text resolved for each filtered commit, not
compose object expressions.

## Inspecting test results

Near the start of the output, `josh compose run` prints the `WS_TREE` SHA:

```
WS_TREE: abc123def456...
```

The scrut-updated `.t` files are stored in the `josh_out_<WS_TREE>` artifact under `tests/`, not in
the working directory.

```sh
# List all test result files
podman volume export josh_out_<WS_TREE> | tar -tvf - tests/

# Print a specific test file to stdout
podman volume export josh_out_<WS_TREE> | tar -xOf - tests/filter/foo.t
```

For failing tests the scrut diff format shows: the shell expression that failed, the expected output (preceded by `-`), and the actual output (preceded by `+`).

Each test file prints a result line, and the final lines of output report the overall result:

```
Result: 1 document(s) with N testcase(s): N succeeded, 0 failed and 0 skipped
SUCCESS: <safe-name>
```

or

```
FAILED: <safe-name>
```

## Cache behavior

Each configured workspace execution can produce a runtime artifact named `josh_out_<WS_TREE>`. Successful and failed result metadata is stored on `refs/josh/compose`, under `success/<AA>/<BBB>/<REST>` and `failed/<AA>/<BBB>/<REST>` respectively, where `<WS_TREE>` is split into the two-character `<AA>`, three-character `<BBB>`, and remaining `<REST>` components. A cached result is reused only when its successful result entry is present and, for workspaces that keep output, the matching output artifact still exists. Command overrides and interactive shells use scratch output and do not update result metadata. The cache key is the Git SHA of the filtered workspace tree, so:

- Changing any file included in the workspace automatically produces a new SHA and bypasses the cache.
- Changing unrelated files has no effect on the cache.
- Two developers with identical workspace contents share the same cache key (useful if volumes are shared via a registry).

### Sharing result metadata

Compose result metadata can be synchronized through the repository's `refs/josh/compose` ref:

```sh
josh compose pull --remote origin
josh compose push --remote origin
```

Both commands default to `origin`. Pull merges remote results with local results. Push retries when another writer advances the remote ref, merging both result trees before retrying. The commands transfer only Git metadata; output volumes use their runtime-specific transport separately.

Each `run` invocation batches its result updates and cache-hit touches into at most one commit on
the result ref.

### Local disk reclamation

Before execution and before each uncached workspace, `run` and `shell` check the runtime
storage usage. At 90% usage, compose removes local `josh_ws_image_*` images and `josh_out_*` output
artifacts in least-recently-used order until usage is at most 80%. Resources required by the pending
graph are protected.

The usage order comes from update and cache-hit commits on `refs/josh/compose`. Automatic
reclamation does not remove persistent `josh_cache_*` volumes, result metadata, or remote objects.

### Clearing compose state

```sh
# Remove output artifacts, images, and result metadata
josh compose clean

# Also remove persistent cache volumes
josh compose clean --all
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
| `:$output="none"` | Disable the workspace output artifact |
| `:$network="host"` | Container network mode |
| `worktree = :[...]` | Files placed in the container's working directory |
| `inputs = :[...]` | Dependency workspaces; each is built first and mounted inside the container |
| `env = :[...]` | Environment variables injected into the container |

The reference-bearing entries created with `:#` (`image`, named `inputs`, image `bases`, and
`sidecars`) are stored as gitlinks. `josh compose` reads their object IDs directly from the tree
entries; the referenced objects remain ordinary josh workspace or image trees.

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

## Image definitions

An image definition accepts `:$label="..."`. The label identifies the image in compose status lines
and `josh compose graph` nodes; without one, these outputs fall back to the image tree OID.

### Using job outputs in image builds

An image definition can declare normal workspaces as named `inputs`:

```
:$label="runtime image"

inputs = :[
    :#josh-binaries[:+ws/build-rust]
]

context = :/images/run
```

Each input workspace must produce an output artifact. Its `/out` directory is exposed to the
Dockerfile as a named build context:

```dockerfile
# syntax=docker/dockerfile:1
FROM alpine
COPY --from=josh-binaries /josh /usr/local/bin/josh
```

Here `/josh` refers to `/out/josh` from `ws/build-rust`. Named contexts also work with
`RUN --mount=from=josh-binaries,...`. Input jobs run only when the image needs building; an existing
image does not require their output artifacts to remain locally available.

## Creating a new workspace

1. **Write a `.josh` file** under `ws/`. Declare at minimum an `:#image[...]` reference and a `worktree` subtree containing the files your build needs.

2. **Write the entrypoint** either as a `run.sh` in the worktree or via `:$cmd="..."`. Place any outputs you want extracted under `/out` inside the container (unless `:$output="none"`).

3. **Run it:**

   ```sh
   josh compose run . :+ws/my-workspace
   ```

   Pass `-- COMMAND...` to execute a one-off or side-effecting command without replacing this result.

4. **Add dependencies** via `inputs = :[...]` if your workspace needs another workspace's output.
   Each dependency is built first and its output artifact is mounted at `/<name>`.
