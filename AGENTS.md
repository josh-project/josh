* Before creating commit, always run `cargo fmt`
* When possible, keep PRs to one commit only; amend existing commit when making changes to PRs, and force push

## Running tests via josh-run/bin/josh-run

Tests run inside an isolated podman container. The cache key is the SHA of the filtered workspace tree, so the cache is automatically invalidated when source files change.

**Run all tests (both release and incubating):**
```
josh-run/bin/josh-run :+ws/test
```

**Run only the release tests (excludes incubating features):**
```
josh-run/bin/josh-run :+ws/test:/deps:*/test-release
```

**Run only the incubating tests:**
```
josh-run/bin/josh-run :+ws/test:/deps:*/test-incubating
```

The second argument to `josh-run/bin/josh-run` is the git ref to build from:
- `.` (default) — working tree including uncommitted changes
- `+` — staged files (git index); useful to test only what you've `git add`ed
- `HEAD` — last commit, ignoring any local changes; useful to compare before/after or do a clean build of the current branch

### Inspecting test output

The summary is printed at the end of `josh-run/bin/josh-run` output:
```
# Ran N tests, M skipped, K failed.
```
Followed by `SUCCESS: <sha>` or `FAILED: <name>`.

The prysk-updated `.t` files (showing diffs for failures) are stored in the `out_<WS_TREE>` podman volume under `tests/`, **not** in the working directory. `josh-run/bin/josh-run` prints the `WS_TREE` SHA near the top of its output. To inspect the test results:

```bash
# List all test files in the out volume
podman volume export out_<WS_TREE> | tar -tvf - tests/

# Print a specific test file to stdout
podman volume export out_<WS_TREE> | tar -xOf - tests/filter/foo.t
```

For failing tests, the prysk diff format shows the command that failed, expected output (indented), and actual output (preceded by `+`).

### Clearing the cache

The output of each run is stored in a podman volume named `out_<WS_TREE_SHA>`. If a volume for the current tree SHA already exists, `josh-run/bin/josh-run` skips execution entirely and just re-exports it.

To force a re-run without changing source files:
```bash
# Find and remove the relevant output volume
podman volume ls | grep out_
podman volume rm out_<sha>
```

To remove all build/test output volumes at once:
```bash
podman volume ls -q | grep '^out_' | xargs podman volume rm
```

The build dependency (`josh-run/bin/josh-run`) has its own separate output volume (prefixed differently). Check `podman volume ls` to see all volumes.

### Iterating on a failing test

1. Edit the `.t` test file or the relevant source code.
2. Re-run `josh-run/bin/josh-run :+ws/test` — the changed working tree produces a new SHA, so the cache is bypassed automatically.
3. If you need to re-run without making any change (e.g. after manually deleting a volume), remove the `out_<sha>` volume as shown above.

Test files live under `tests/` and are organized by subsystem:
- `tests/filter/` — filter language tests (largest suite; `incubating_*.t` requires `--features incubating`)
- `tests/proxy/` — git proxy tests
- `tests/cli/` — CLI tests
- `tests/experimental/` — excluded from release tests
