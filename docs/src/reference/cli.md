# josh CLI

The `josh` command is the primary tool for working with filtered git repositories.
It provides projection-aware equivalents of the common git operations.

## Installation

```shell
cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
```

---

## josh clone

Clone a repository, optionally applying a filter projection.

```
josh clone <url> <filter> <out> [options]
```

| Argument | Description |
|----------|-------------|
| `<url>`  | Remote repository URL (HTTPS, SSH, or local path) |
| `<filter>` | [Filter spec](./filters.md) to apply (e.g. `:/docs`, `:workspace=workspaces/myproject`) |
| `<out>`  | Local directory to clone into |

**Options:**

| Flag | Description |
|------|-------------|
| `-b`, `--branch <ref>` | Branch or ref to clone (default: `HEAD`) |
| `--forge <name>` | Forge integration to use (e.g. `github`) |
| `--no-forge` | Disable forge integration |

**Examples:**

```shell
# Clone only the docs/ subdirectory
josh clone https://github.com/josh-project/josh.git :/docs ./josh-docs

# Clone a workspace projection
josh clone https://github.com/myorg/monorepo.git :workspace=workspaces/frontend ./frontend

# Clone the full repository (no filter)
josh clone https://github.com/josh-project/josh.git :/ ./josh
```

---

## josh fetch

Fetch from a remote and update the filtered local refs. Equivalent to `git fetch` but
filter-aware.

```
josh fetch [options]
```

**Options:**

| Flag | Description |
|------|-------------|
| `-r`, `--remote <name>` | Remote name or URL to fetch from (default: `origin`) |
| `-R`, `--ref <ref>` | Ref to fetch (default: `HEAD`) |

---

## josh pull

Fetch and integrate changes from a remote. Equivalent to `git pull` but filter-aware.

```
josh pull [options]
```

**Options:**

| Flag | Description |
|------|-------------|
| `-r`, `--remote <name>` | Remote name or URL to pull from (default: `origin`) |
| `-R`, `--ref <ref>` | Ref to pull (default: `HEAD`) |
| `--rebase` | Rebase the current branch on top of the upstream branch |
| `--autostash` | Automatically stash local changes before rebasing |

---

## josh push

Push commits back to the upstream repository. Josh reverses the filter and reconstructs
the correct upstream commits, so your changes land in the right place in the monorepo.

```
josh push [<remote>] [<refspecs>...] [options]
```

| Argument | Description |
|----------|-------------|
| `<remote>` | Remote name to push to (default: `origin`) |
| `<refspecs>` | Refs to push (default: current branch) |

**Options:**

| Flag | Description |
|------|-------------|
| `-f`, `--force` | Force-push (non-fast-forward) |
| `--atomic` | Atomic push (all-or-nothing) |
| `--dry-run` | Show what would be pushed without actually pushing |

---

## josh publish

Push each commit as an independent, minimal diff (stacked changes workflow). Each commit
with a [Change ID](../guide/stacked-changes.md) is pushed to its own ref and, when
[forge integration](./forge.md) is configured, gets its own pull request.

```
josh publish [<remote>] [<refspecs>...] [options]
```

| Argument | Description |
|----------|-------------|
| `<remote>` | Remote name to push to (default: `origin`) |
| `<refspecs>` | Refs to push (default: current branch) |

**Options:**

| Flag | Description |
|------|-------------|
| `-f`, `--force` | Force-push (non-fast-forward) |
| `--atomic` | Atomic push (all-or-nothing) |
| `--dry-run` | Show what would be pushed without actually pushing |

---

## josh remote

Manage josh-aware remotes.

> **Note**: `josh remote add` can be used in any existing git repository, not only ones
> originally cloned with `josh clone`. This is the standard way to add josh filtering to
> a repository you already have checked out.

### josh remote add

Add a remote with an associated filter projection.

```
josh remote add <name> <url> <filter> [options]
```

| Argument | Description |
|----------|-------------|
| `<name>` | Remote name |
| `<url>`  | Remote repository URL |
| `<filter>` | [Filter spec](./filters.md) to associate with this remote |

**Options:**

| Flag | Description |
|------|-------------|
| `--forge <name>` | Forge integration (e.g. `github`) |
| `--no-forge` | Disable forge integration |

**Example:**

```shell
# Add a second remote scoped to the backend/ subdirectory
josh remote add backend https://github.com/myorg/monorepo.git :/services/backend
```

---

## josh filter

Re-apply the filter for an existing remote to update the local filtered refs. Useful
after manually modifying the filter configuration without fetching.

```
josh filter <remote>
```

| Argument | Description |
|----------|-------------|
| `<remote>` | Remote name whose filter should be re-applied |

---

## josh auth

Manage authentication credentials for forge integrations. Forge integration is optional
and used for automatic pull request management — see
[Forge integration](./forge.md) for details.

```
josh auth login <forge>
josh auth logout <forge>
```

The only currently supported forge is `github`. See
[Forge integration](./forge.md) for full documentation.

---

## josh cache

Manage the distributed filter cache. The distributed cache stores filter results inside
a ref in the git repository, allowing a warm cache to be shared between machines via
ordinary git push/fetch.

The cache subcommand requires a josh remote to be configured (see `josh remote add`).

### josh cache build

Apply the configured filter to all already-fetched refs and populate the local distributed
cache with the results.

```
josh cache build [remote]
```

| Argument | Description |
|----------|-------------|
| `[remote]` | Remote name to build cache for (default: `origin`) |

Run this before `josh cache push` to ensure the cache is up to date.

### josh cache push

Push the local distributed cache and the filtered refs to the backing remote, so that
other machines can fetch them.

```
josh cache push [remote]
```

| Argument | Description |
|----------|-------------|
| `[remote]` | Remote name to push cache to (default: `origin`) |

### josh cache fetch

Fetch the distributed cache and filtered refs from the remote, warming the local cache
without re-computing filters from scratch.

```
josh cache fetch [remote]
```

| Argument | Description |
|----------|-------------|
| `[remote]` | Remote name to fetch cache from (default: `origin`) |

**Typical workflow:**

```shell
# On the machine that computes the cache (e.g. CI):
josh cache build
josh cache push

# On another machine (e.g. a developer workstation):
josh cache fetch
# subsequent josh fetch / clone operations use the pre-built cache
```

> **Note:** The distributed cache is currently only available through the `josh` CLI.
> It is not yet supported by `josh-proxy`.

---

## josh-filter (standalone binary)

`josh-filter` is a lower-level command that rewrites git history using Josh filter specs.
It is intended for scripting and one-off history rewriting tasks rather than day-to-day
development workflows.

By default it reads from `HEAD` and writes the filtered result to `FILTERED_HEAD`.

**Basic usage:**

```shell
# Filter HEAD through :/docs and write result to FILTERED_HEAD
josh-filter :/docs
```

**Options:**

| Flag | Description |
|------|-------------|
| `--file <path>` | Read filter spec from a file |
| `--squash-pattern <pattern>` | Squash commits matching the pattern |
| `--squash-file <path>` | Read squash patterns from a file |
| `--single` | Produce a single squashed commit |
| `-d` | Discovery mode: populate cache with probable filters |
| `-t` | Output Chrome tracing data |
| `-p` | Print the filter spec |
| `-i` | Print the filter ID |
| `--cache-stats` | Print cache statistics |
| `--reverse` | Swap input and output (unapply filter) |
