# josh-run

`josh-run` is a containerized build and test execution tool that uses josh filters to create minimal, isolated, and automatically-cached workspaces. It is used to build the josh binaries and run the integration test suite.

## Motivation

A common problem with monorepo builds is that the container context contains the entire repository — thousands of files most builds don't need. This has two consequences:

1. **Fragile caching.** Docker and podman derive cache tags from the context contents, but you have to be careful to exclude unrelated files. Any file touched anywhere in the repo can break the cache even if the build doesn't use that file.
2. **Hidden dependencies.** If a build silently reads a file that "happens to be there", it works locally but may break in a clean environment or for another team member.

`josh-run` addresses both problems by using `josh-filter` to reduce the repository to exactly the files a given workspace needs before any container is involved:

- The filtered tree's git SHA becomes the cache key. Only changes to files actually included in the workspace invalidate the cache.
- The container context is a plain `git archive` of the filtered tree — no bind-mounts, no extra paths. Files outside the workspace are structurally impossible to access.
- The host working tree is never modified. Artifacts produced by the container are stored in a podman volume and exported back when the run completes.

## Prerequisites

- **podman** installed and on `$PATH`
- **`josh-filter`** built and on `$PATH`. Install it first:

  ```sh
  cargo install --path ./josh-cli --force --locked
  ```

## Quick start

All commands are run from the root of the josh repository.

### Build the josh binaries

```sh
josh-run/bin/josh-run :+ws/build
```

### Run all tests

```sh
josh-run/bin/josh-run :+ws/test
```

This runs both the release test suite and the experimental test suite (with `JOSH_EXPERIMENTAL_FEATURES=1`).

### Run only release tests

```sh
josh-run/bin/josh-run :+ws/test:/deps:*/stable
```

### Run only experimental tests

```sh
josh-run/bin/josh-run :+ws/test:/deps:*/experimental
```

## Syntax

```
josh-run/bin/josh-run <filter> [<ref>]
```

| Argument | Description |
|---|---|
| `<filter>` | A josh filter expression selecting the workspace to run (e.g. `:+ws/test`). `:SQUASH` is prepended automatically. |
| `<ref>` | The git ref to build from. Defaults to `.` (working tree). |

### `<ref>` values

| Value | Meaning |
|---|---|
| `.` (default) | Working tree, including uncommitted changes |
| `+` | Staged files only (git index). Useful to test exactly what you have `git add`ed. |
| `HEAD` | Last commit, ignoring any local changes. Useful for clean builds or before/after comparisons. |
| Any git ref or SHA | Build from that specific commit. |

When `<ref>` is `.` or `+`, `josh-run` prints a `git diff --stat` summary before running so you can see what is included.

## Inspecting test results

Near the start of the output, `josh-run` prints the `WS_TREE` SHA:

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

Each run produces a podman volume named `out_<WS_TREE>`. If that volume already exists when `josh-run` is invoked, the entire build/test step is skipped and the cached output is re-exported directly. The cache key is the git SHA of the filtered workspace tree, so:

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

### Clearing all output volumes

```sh
podman volume ls -q | grep '^out_' | xargs podman volume rm
```

Note: build and test runs have separate output volumes. `podman volume ls` lists all of them.

## Workspace definitions

A workspace is defined by a josh filter file, typically under `ws/`. The filter selects a tree with a specific directory structure that `josh-run-container` interprets. The subdirectories of the workspace tree have the following roles:

| Path | Type | Purpose |
|---|---|---|
| `image` | blob (reference to another tree) | Points to the image workspace used to build the container image |
| `run/` | subtree | Files placed in the container. Must include `run.sh` as the entrypoint. |
| `deps/` | subtree | Each file names a dependency. Its content is the tree SHA of that dependency's output volume, which is mounted at `/<filename>` inside the container. |
| `cache` | blob | Optional. Names a persistent podman volume mounted at `/opt/cache` (e.g. for Cargo's registry cache). |
| `env/` | subtree | Optional. Each file is an environment variable injected into the container. The filename is the variable name; the content is the value. |
| `output` | blob | Optional. `none` disables the output volume entirely, so no `/out` mount is provided and the run is never cache-skipped based on a prior output volume. |

### Example: `ws/build.josh`

```
:#image[:+images/dev-local/image]
:$cache="cargo"

run = :[
    ::run.sh=ws/build.sh
    :exclude[
        ::ws/
        ::tests/
    ]
]
```

This workspace:
- Uses the `images/dev-local/image` subtree as the Docker build context.
- Declares a persistent cache volume named `cargo` (mounted at `/opt/cache`).
- Populates `run/` with: the build entrypoint (`ws/build.sh` renamed to `run.sh`) and every file in the repo except `ws/` and `tests/`.

### Example: `ws/test.josh`

```
deps = :[
    :#stable[
        :#image[:+images/dev-local/image]

        deps = :[
            :#josh[:+ws/build-rust]
            :#build-go[:+ws/build-go]
        ]

        run = :[
            ::run.sh=ws/tests.sh
            ::run-tests.sh
            ::tests/:exclude[::tests/experimental/]
            ::scripts/
        ]
    ]
    :#experimental[
        :#image[:+images/dev-local/image]

        deps = :[
            :#josh[:+ws/build-rust]
            :#build-go[:+ws/build-go]
        ]

        env = :[
            :$JOSH_EXPERIMENTAL_FEATURES="1"
        ]

        run = :[
            ::run.sh=ws/tests.sh
            ::run-tests.sh
            ::tests/
            ::scripts/
        ]
    ]
]
```

This workspace has two top-level dependencies: `stable` and `experimental`. Each:
- Uses `images/dev-local/image` as the container image.
- Depends on the `ws/build-rust` and `ws/build-go` workspaces (whose outputs are mounted inside the test container).
- `experimental` sets `JOSH_EXPERIMENTAL_FEATURES=1` in the container environment, enabling experimental features at runtime.
- `stable` excludes `tests/experimental/` from the run tree.

## How it works

`josh-run` is composed of three scripts.

```
josh-run/bin/josh-run <filter> [<ref>]
│
│  1. Prepend :SQUASH to filter
│  2. Run josh-filter to get filtered tree SHA → WS_TREE
│  3. Derive a safe name from the filter (josh-filter -i)
│
└─► josh-run-container <safe_name> <WS_TREE>
    │
    │  4. If output != none, check if out_<WS_TREE> volume exists → cache hit, skip to step 11
    │  5. For each entry in WS_TREE:deps/:
    │       a. Read the dep's WS_TREE SHA from WS_TREE:deps/<name>
    │       b. Recurse: josh-run-container <safe_name>-<dep> <dep_sha>
    │       c. Add -v out_<dep_sha>:/<dep_name> to podman run args
    │  6. Read WS_TREE:image → IMAGE_TREE
    │
    └─► josh-run-build-image <IMAGE_TREE>
        │
        │  7. Check if ws_image_<IMAGE_TREE> exists → skip if yes
        │  8. git archive IMAGE_TREE:context | podman build → ws_image_<IMAGE_TREE>
        │
    ◄──
    │  9.  git archive WS_TREE:run | podman volume import → snapshot volume
    │  10. podman run ws_image_<IMAGE_TREE> bash run.sh
    │       - snapshot volume mounted at $PWD
    │       - out_<WS_TREE> volume mounted at /out when output != none
    │       - dep output volumes mounted at /<dep_name>
    │       - cache volume mounted at /opt/cache (if configured)
    │  11. podman volume rm snapshot volume
    │
◄──
    12. If output != none, podman volume export out_<WS_TREE> | tar -xvf -
        (extract artifacts to host working directory)
```

### Image naming

Container images are named `ws_image_<IMAGE_TREE>` where `IMAGE_TREE` is the git SHA of the image workspace's `context/` subtree. This means the image is rebuilt only when its Dockerfile or supporting files actually change.

### Artifact extraction

After the run, `josh-run` exports the entire `out_<WS_TREE>` volume back to the current working directory with `tar -xvf -` when output collection is enabled. The entrypoint script (`run.sh`) is responsible for placing outputs under `/out` inside the container. For example, `ws/build.sh` copies compiled binaries to `/out/debug/`.

## Creating a new workspace

1. **Write a `.josh` file** under `ws/` (or anywhere in the repo). Define at minimum an `image` reference and a `run` subtree containing a `run.sh` entrypoint.

2. **Write the entrypoint script** (`run.sh`). It runs inside the container. Place any outputs you want extracted under `/out`, unless you set `output = "none"` for a run-only workspace.

3. **Run it:**

   ```sh
   josh-run/bin/josh-run :+ws/my-workspace
   ```

   The filter `:+ws/my-workspace` selects the tree at `ws/my-workspace.josh` evaluated against the repository.

4. **Add dependencies** if your workspace needs the output of another workspace. Add a `deps` subtree where each entry is a file whose name is the mount point inside the container and whose content is the SHA of the dependency's workspace tree. In practice this is done via josh filter composition, as shown in `ws/test.josh`.
