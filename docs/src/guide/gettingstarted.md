# Getting Started

The `josh` command-line tool creates filtered worktrees backed by one shared history store.
Git handles local work; Josh synchronizes the store with upstream repositories.

## Installation

Install `josh` using Cargo (requires [Rust](https://rustup.rs)):

```shell
cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
```

## Creating a filtered worktree

Clone an upstream into a filtered worktree:

```shell
josh clone https://github.com/josh-project/josh.git docs --filter :/docs
```

The filter `:/docs` gives the `docs` worktree only the contents and history of the
upstream `docs/` directory. Josh keeps fetched history and projection caches in a shared
per-user store under the platform's application-data directory. Set `JOSH_STORE` or pass
`--store <path>` to use another location.

Additional worktrees can live anywhere and reuse the store's objects, network fetches, and
projection cache:

```shell
josh worktree add ../full :/github.com/josh-project/josh
josh worktree add ../core :/github.com/josh-project/josh/josh-core
```

## Making and pushing changes

A Josh worktree is a normal Git repository. Edit, commit, branch, and rebase with Git.
Its Git remotes point to namespace-isolated refs in the local store, so plain `git fetch`
and `git push` are always local:

```shell
cd docs
# ... edit files, git add, git commit ...
git push
```

Relay the parked branch to the actual upstream with Josh:

```shell
josh push
```

Josh reverses the mount and worktree filters when relaying a change, so edits
from this checkout land under `docs/` in the unfiltered upstream repository.

## Pulling changes

Fetch the upstream into the store, refresh all worktrees, and integrate the current branch:

```shell
josh pull
```

Use `josh changes pull --remote upstream` for change-aware stacked-history integration.

## Working on part of a repository

The CLI fetches the full upstream object database into the shared store and filters there.
The worktree's filter controls visible files, history, refs, and reverse-filtered pushes;
it does not reduce the initial network transfer.

> **Note**: If a true partial download is important — for example to avoid transferring
> a large monorepo over a slow connection — you need server-side filtering via a
> [josh-proxy](../reference/proxy.md). With the proxy in place, only the filtered
> objects are ever sent over the network.

Beyond simple subdirectory extraction, Josh's
[filter language](../reference/filters.md) supports composition, remapping, and
exclusions, making it possible to carve out any virtual slice of a repository.

## Next steps

- **[Workspaces](./workspaces.md)** — Compose a virtual repository from multiple parts
  of a monorepo and keep them in sync bidirectionally.
- **[Stacked changes](./stacked-changes.md)** — Push a series of commits as individual
  pull requests with automatic PR management.
- **[Filter syntax](../reference/filters.md)** — Learn all the available filter
  operations.
- **[josh CLI reference](../reference/cli.md)** — Full reference for all `josh`
  subcommands and options.
- **[Proxy setup](../reference/proxy.md)** — Running a shared `josh-proxy` for your
  team or CI/CD infrastructure, so that ordinary `git clone` works without any special
  client tooling.
