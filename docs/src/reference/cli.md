# josh CLI

The `josh` command manages a shared history store and filtered linked repositories.
Git handles local worktree operations; Josh fetches from and pushes to upstream repositories.

## Installation

```shell
cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
```

## Shell completion

`josh` and `josh-filter` provide dynamic completion for commands, options, filter
expressions, repository paths, and Git refs. Register both binaries in the shell's
startup file.

For Zsh:

```zsh
source <(COMPLETE=zsh josh)
source <(COMPLETE=zsh josh-filter)
```

Zsh treats `!` as history expansion. When entering a Starlark filter prefix
manually, escape it as `:\!`; completion preserves the escape while matching
repository `.star` files.

For Bash, use the same commands with `bash` in place of `zsh`. For Fish:

```fish
COMPLETE=fish josh | source
COMPLETE=fish josh-filter | source
```

For Elvish:

```elvish
eval (E:COMPLETE=elvish josh | slurp)
eval (E:COMPLETE=elvish josh-filter | slurp)
```

For PowerShell:

```powershell
$env:COMPLETE = "powershell"; josh | Out-String | Invoke-Expression; Remove-Item Env:\COMPLETE
$env:COMPLETE = "powershell"; josh-filter | Out-String | Invoke-Expression; Remove-Item Env:\COMPLETE
```

Repository-aware completion reads local refs and trees. It does not contact remotes.

---

## Store selection

Commands use the store associated with the current worktree or store directory. Outside one,
Josh uses `JOSH_STORE` when set and otherwise keeps a per-user store in the platform's local
application-data directory. Pass the global `--store <path>` option to select another store
explicitly; it takes precedence over the current worktree.

`josh clone` and `josh remote add` initialize the selected store when it does not exist.

---

## josh clone

Attach an upstream and create a filtered worktree in one command:

```shell
josh clone <url> [<directory>] [--filter <filter>] [--mount <path>] [-b <ref>]
```

The directory defaults to the repository name. The filter is relative to the upstream
repository root and defaults to the complete repository. GitHub mounts include the host, owner, and
repository—for example, `https://github.com/rust-lang/rust.git` is mounted at
`github.com/rust-lang/rust`. Other remotes default to the repository name. `--mount` overrides
the discovered path.

If the selected store already contains the same URL at that mount, Josh reuses its fetched
history and creates another projection. A different URL at the same mount is rejected.

```shell
josh clone https://github.com/josh-project/josh.git docs --filter :/docs
```

---

## josh init

Explicitly create the selected store:

```shell
josh [--store <path>] init
```

Most users do not need this command because `clone` and `remote add` initialize the store.

---

## josh worktree

Create, inspect, and remove linked repositories backed by a store:

```shell
josh worktree add <path> [<filter>] [-b <branch>] [--ref <ref>]
josh worktree add <path> --submodules --remote <superproject>
josh worktree list
josh worktree remove <path> [--force]
```

`worktree add` defaults to the identity filter (`:/`) over the composed store.
Each upstream repository is mounted below its declared path. A worktree filter
then selects or reshapes that composed tree. Worktree paths are resolved from the current
directory and may be anywhere on the filesystem. Each linked repository has its own branches
and configuration while sharing the store's objects through a Git alternate. Its Git remotes
point to namespace-isolated refs in the local store, so ordinary `git fetch` and `git push`
never contact an upstream.

The private `refs/josh/super` history is authoritative for store configuration, fetched
upstream refs, and shared change refs. Namespace, backing, and worktree refs are derived
materializations and can be rebuilt from that history.

With experimental features enabled, `--submodules` derives an inlining filter and
export links from the selected superproject remote. Inlining is worktree state; it
does not change any remote mount.

```shell
josh clone https://github.com/josh-project/josh.git docs --filter :/docs
josh worktree add full :/github.com/josh-project/josh
```

---

## josh fetch

Fetch upstream history into the store, materialize every projection, and refresh
registered worktrees' remote-tracking refs.

```shell
josh fetch [--remote <identity>]
```

All upstreams are fetched when `--remote` is omitted.

---

## josh push

Relay the current branch (or explicit refspecs) from a worktree through the store to an
upstream. A push within one projection is unchanged; a push to its parent projection reverses
the worktree filter.

```shell
josh push [<remote>] [<refspecs>...] [options]
```

When the store has one remote it is selected automatically. With multiple remotes, the
current branch's tracking remote is used unless a remote is explicit.

| Flag | Description |
|------|-------------|
| `-f`, `--force` | Force-update the store and upstream branch |
| `--atomic` | Request an atomic upstream push |
| `--dry-run` | Park the branch locally, but do not update the upstream |
| `--base <branch>` | Use another upstream branch as the reverse-filter base |
| `--merge` | Wrap the reverse-filtered result in a merge commit |

---

## josh pull

Run `josh fetch` for one upstream, then delegate branch integration to
`git pull --rebase --autostash` in the current worktree.

```shell
josh pull --remote <identity> [--ref <ref>]
```

---

## josh migrate

Convert a clean old-style, filter-bearing clone into a store with one linked worktree.
Local branches are preserved.

```shell
josh migrate [<directory>] [--worktree <name>]
```

The linked worktree name defaults to `worktree`.

## josh changes publish

Push each commit as an independent, minimal diff (stacked changes workflow). Each commit
with a [Change ID](../guide/stacked-changes.md) is pushed to its own ref and, when
[forge integration](./forge.md) is configured, gets its own pull request.

The push uses `git push --porcelain` and prints a curated summary instead of git's
per-ref noise: a single `published N changes (M new)` line for the stacked-changes
refs, with the internal `@base/…`/`@heads/…` refs hidden. Git's stderr (progress,
rejection details) is still forwarded verbatim. When a pull request is created or
updated and stderr is a terminal, the `PR #N` reference is printed as a clickable
hyperlink (OSC 8; supported by iTerm2, kitty, WezTerm and others).

```
josh changes publish [<remote>] [<refspecs>...] [options]
```

| Argument | Description |
|----------|-------------|
| `<remote>` | Mount-derived remote identity; inferred from branch tracking or a sole store remote |
| `<refspecs>` | Refs to push (default: current branch) |

**Options:**

| Flag | Description |
|------|-------------|
| `-f`, `--force` | Force-push (non-fast-forward) |
| `--atomic` | Atomic push (all-or-nothing) |
| `--dry-run` | Show what would be pushed without actually pushing |

---

## josh changes pull

Fetch and integrate changes from a remote (stacked changes workflow). Like `git pull
--rebase --autostash`, but filter-aware and change-aware.

The fetch uses `git fetch --porcelain` (requires git >= 2.41) and prints a curated
summary instead of git's per-ref noise: one line per updated branch, plus a change
count for stacked-changes refs (`@changes/…`). A republished change only counts as
updated when its content actually changed — compared via `git patch-id --stable` —
so restacking without edits does not inflate the count.

Integration is rebase-style and linear-only:

- If the local branch can be fast-forwarded, it is fast-forwarded.
- Local commits whose `Change-Id` trailer already appears in the updated upstream
  history (e.g. landed by a merge queue) are skipped; the remaining changes are
  restacked (cherry-picked) on top of the upstream tip.
- Uncommitted changes are stashed before integrating and re-applied afterwards
  (autostash).
- Anything more complex — merge commits in the local stack, diverged commits
  without a `Change-Id`, or restack conflicts — fails with an error and leaves the
  branch untouched.

```
josh changes pull [options]
```

**Options:**

| Flag | Description |
|------|-------------|
| `-r`, `--remote <identity>` | Mount-derived identity; inferred from branch tracking or a sole store remote |
| `-R`, `--ref <ref>` | Ref to pull (default: `HEAD`) |

---

## josh remote add

Attach an upstream at a path in the selected store and fetch it immediately. The store is
initialized first when necessary.

```shell
josh remote add <path> <url> [options]
```

`<path>` is a non-root normalized relative path such as `rust` or `tools/miri`; it is not a
Josh filter. Josh mounts the upstream repository root beneath that path in the composed store.
The mount path is also the remote's identity. Single-component paths remain readable Git remote
names, while nested paths are percent-encoded where Git requires it. Remote mount paths must be
pairwise disjoint: equal, ancestor, and descendant paths are rejected.

| Flag | Description |
|------|-------------|
| `--submodules` | Also discover configured submodules and add them at `<submodule-name>` |
| `--push-url <url>` | Separate branch-push destination, usually a fork |
| `--forge <name>` | Forge integration (for example, `github` or `gerrit`) |
| `--no-forge` | Disable forge integration |
| `--gerrit-mode <mode>` | Gerrit publication mode; see [Gerrit publish modes](forge.md#publish-modes) |

```shell
josh remote add rust https://github.com/rust-lang/rust
josh remote add miri https://github.com/rust-lang/miri
josh worktree add rust-work :/rust
josh worktree add miri-work :/miri
```

A full worktree sees each remote's branches with trees mounted under their respective
paths. A worktree selecting exactly one mount sees that upstream repository at its root.

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

Manage the store's distributed filter cache. Cache refs can be pushed to an upstream and
fetched by another store.

### josh cache build

Apply known materialized filters and populate the distributed cache:

```shell
josh cache build <remote> [--filter <filter>]
```

`--filter` explicitly selects a worktree projection; otherwise Josh uses
materialized projections and the remote's mount filter.

### josh cache push

Push cache refs and the materialized refs needed by a projection:

```shell
josh cache push <remote> [--filter <filter>]
```

### josh cache fetch

Fetch shared cache and materialization refs:

```shell
josh cache fetch <remote> [--filter <filter>]
```

Typical cross-machine workflow:

```shell
josh cache build upstream --filter :/repo/docs
josh cache push upstream --filter :/repo/docs

# In another store with the same :/repo mount:
josh cache fetch upstream --filter :/repo/docs
josh fetch --remote upstream
```

> **Note:** The distributed cache is currently only available through the `josh` CLI.
> It is not yet supported by `josh-proxy`.

---

## josh compose

> **Experimental:** requires `JOSH_EXPERIMENTAL_FEATURES=1`.

Compose prepares filtered workspaces in isolated containers. See
[josh compose](../contributing/josh-compose.md) for workspace definitions and cache behavior.

### josh compose run

Run the selected graph, reusing successful configured results. A command after `--` always executes
in the selected workspace without replacing its configured result or output artifact.

```text
josh compose run [OPTIONS] [FILTER] [-- COMMAND...]
```

### josh compose shell

Prepare the selected workspace and open an interactive `/bin/sh`. A command after `--` replaces the
shell. Interactive execution uses scratch output and does not update the configured result.

```text
josh compose shell [OPTIONS] [FILTER] [-- COMMAND...]
```

`-r REVISION`/`--revision REVISION` selects the Git input and defaults to `.` (the working tree);
`+` selects the index. `FILTER` defaults to `:+compose`.

Common options:

| Flag | Description |
|------|-------------|
| `--backend BACKEND` | Select `podman` or `docker` |
| `-r, --revision REVISION` | Select the Git input; defaults to `.` |
| `--arg NAME=VALUE` | Bind a named revision argument; repeatable |

```shell
# Run the default workspace defined by compose.josh
josh compose run

# Execute a side-effecting command without replacing the configured result
josh compose run :+ws/action -- ./perform-action

# Inspect the prepared environment interactively
josh compose shell :+ws/build-rust
```

### josh compose clean

Remove cached output artifacts, images, and result metadata. `--all` also removes persistent cache
volumes.

```text
josh compose clean [--all] [--backend BACKEND]
```

### josh compose graph

Print the complete workspace plan as D2 source without executing containers.

```text
josh compose graph [OPTIONS] [FILTER]
```

`--revision` and `FILTER` have the same meaning and defaults as the execution commands.

```shell
josh compose graph -r HEAD :+ws/test > graph.d2
```

---


## josh-filter (standalone binary)

`josh-filter` is a lower-level command that rewrites git history using Josh filter specs.
It is intended for scripting and one-off history rewriting tasks rather than day-to-day
development workflows.

**Input:** by default, the second positional argument selects one revision to filter and
`--update` names its destination. The input defaults to `HEAD` but can be any of:

- `.` - the working tree (including uncommitted changes)
- `+` - the index (staged changes only)
- A full or abbreviated commit SHA
- A ref name (e.g. `main`, `refs/heads/feature`)

With `--revspec <source>:<destination>`, `josh-filter` instead filters every ref matched by
the Git-style refspec and writes each result to its mapped destination. Wildcard refspecs
and repeated `--revspec` options are supported.

**Output:** single-revision mode prints the filtered commit SHA. Refspec mode prints one
`<SHA> <destination-ref>` line per updated ref.

**Basic usage:**

```shell
# Filter HEAD through :/docs and write result to FILTERED_HEAD
josh-filter :/docs

# Filter the working tree and print the resulting SHA
josh-filter :/docs .

# Filter a specific commit SHA
josh-filter :/docs abc1234 --update refs/my/filtered

# Filter every local branch into refs/filtered/heads/
josh-filter :/docs --revspec 'refs/heads/*:refs/filtered/heads/*'
```

**Options:**

| Flag | Description |
|------|-------------|
| `--update <ref>` | Ref to update with the filtered result (default: `FILTERED_HEAD`) |
| `--revspec <source>:<destination>` | Filter all matching refs into mapped destination refs; may be repeated |
| `--file <path>` | Read filter spec from a file |
| `--squash-pattern <pattern>` | Squash commits matching the pattern |
| `--squash-file <path>` | Read squash patterns from a file |
| `--single` | Produce a single squashed commit |
| `-d` | Discovery mode: populate cache with probable filters |
| `-t` | Output Chrome tracing data |
| `-p` | Print the filter spec (and exit) |
| `-i` | Print the filter ID (and exit) |
| `-s` | Print cache statistics |
| `-n` | Skip loading the cache |
| `--distributed-cache` | Enable the distributed cache backend |
| `--reverse` | Reverse-apply the filter (unapply): reconstruct upstream commits from filtered ones |
| `--check-roundtrip` | When used with `--reverse`, verify that applying the filter to the reverse result reproduces the original commit. Exits with code 1 if the check fails. |
