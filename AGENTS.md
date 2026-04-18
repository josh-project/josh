* Before creating commit, always run `cargo fmt`
* When possible, keep PRs to one commit only; amend existing commit when making changes to PRs, and force push
* All files (source code, Markdown, text, etc.) should be wrapped at 100 columns max, as long as
  the syntax allows it (e.g. don't break URLs or code blocks that must be on one line)
* Use an `Assisted-By:` footer (not `Co-Authored-By:`) to attribute LLM/agent involvement in commits
* Commit messages must not use a conventional commits prefix (e.g. no `fix:` or `feat:`)
* Commit subject line must start with an uppercase letter and be under 79 characters
* Commit messages must include a `Change:` footer with an alphanumeric, dash-separated identifier
  (e.g. `Change: flatten-invert-check`)
* The `Assisted-By:` footer must reference the actual model used, not a generic name

## Running tests via "josh compose run"

Tests run inside an isolated podman container. The cache key is the SHA of the filtered workspace tree, so the cache is automatically invalidated when source files change.

**Run all tests:**
```
josh compose run
```

To test a specific commit instead of the working tree, pass it as the first argument:
```
josh compose run HEAD
```

Common refs:
- `.` (default) — working tree including uncommitted changes
- `+` — staged files (git index); useful to test only what you've `git add`ed
- `HEAD` — last commit, ignoring any local changes

### Inspecting test output

The summary is printed at the end of `josh compose run` output:
```
# Ran N tests, M skipped, K failed.
```
Followed by `SUCCESS: <sha>` or `FAILED: <name>`.

For failing tests, the prysk diff format shows the command that failed, expected output (indented), and actual
output (preceded by `+`). The updated `.t` files are written back to the working directory, so you can inspect
them directly.

### Iterating on a failing test

1. Edit the `.t` test file or the relevant source code.
2. Re-run `josh compose run` — the changed working tree produces a new SHA, so the cache is bypassed automatically.

Test files live under `tests/` and are organized by subsystem:
- `tests/filter/` — filter language tests (largest suite)
- `tests/proxy/` — git proxy tests
- `tests/cli/` — CLI tests
- `tests/experimental/` — excluded from release tests
