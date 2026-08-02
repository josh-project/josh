---
name: josh-compose
description: Run integration tests in this repo using "josh compose" tool. Use when the user requests to run integration tests.
---

"josh compose" is a subcommand of "josh" CLI that runs a set of commands in defined environments. The commands
and environments are defined in `.josh` files, most of which are in the root of the repo in the `/ws` directory.

## Obtaining access to `josh` binary

When `josh` is available in current environment, use that command. When it's not available, use
`cargo run --bin josh -- compose ...` to run the command. 

## Test organization

This repo uses `scrut` tool to run integration tests. `scrut` is a tool for snapshot testing.
The tool takes cram-style .t files where shell commands are defined along with their expected output.
When the output changes, `scrut` detects it.

Test files live under `tests/` and are organized by subsystem:

- `tests/filter/` — filter language tests (largest suite)
- `tests/proxy/` — git proxy tests
- `tests/cli/` — CLI tests
- `tests/experimental/` — excluded from release tests

## Running tests via `josh compose run`

Tests run inside an isolated podman container. Step results are cached; the cache key is the SHA of
the filtered workspace tree, so the cache is automatically invalidated when source files change.

### Running all tests

```
josh compose run
```

To test a specific commit instead of the working tree, pass it as the first argument:

```
josh compose run HEAD
```

Other values you can pass include:

- `.` (default) — working tree, including uncommitted changes
- `+` — staged files (git index); useful to test only what you've `git add`ed
- `HEAD` — last commit, ignoring any local changes

### Inspecting test output

Each test file prints a result line in the `josh compose run` output:
```
Result: 1 document(s) with N testcase(s): N succeeded, 0 failed and 0 skipped
```
Followed at the end by `SUCCESS: <sha>` or `FAILED: <name>`.

For failing tests, the scrut diff format shows the shell expression that failed, expected output (preceded
by `-`), and actual output (preceded by `+`). The updated `.t` files are written back to the working
directory, so you can inspect them directly.

### Iterating on a failing test

1. Edit the `.t` test file or the relevant source code.
2. Re-run `josh compose run` — the changed working tree produces a new SHA, so the cache is bypassed automatically.

### Do not use --clean or --clean-all

Never pass `--clean` or `--clean-all` to `josh compose run`. The cache is reliable; clearing it just forces
a full rebuild and wastes time. If something seems wrong that you believe a cache wipe would fix,
stop and ask the user to run the clean in a separate terminal — do not run it yourself, ever.
