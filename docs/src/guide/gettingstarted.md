# Getting Started

The `josh` command-line tool lets you clone, fetch, push, and manage filtered views of
git repositories directly from your terminal.

## Installation

Install `josh` using Cargo (requires [Rust](https://rustup.rs)):

```shell
cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
```

## Cloning a repository

`josh clone` is similar to `git clone` but takes two required arguments after the URL:
a [filter](../reference/filters.md) and a local destination path. Unlike `git clone`,
the destination path is always required and cannot be inferred from the URL.

For example, let's clone just the documentation folder of the Josh repository:

```shell
josh clone https://github.com/josh-project/josh.git :/docs ./josh-docs
```

The filter `:/docs` tells Josh to check out only the contents of the `docs/` subdirectory.
The resulting repository will contain only the files from that folder and only the commits
that touch them — as if that subdirectory had always been its own repository.

To clone a repository without any filter (equivalent to a plain `git clone`):

```shell
josh clone https://github.com/josh-project/josh.git :/ ./josh
```

## Making and pushing changes

The cloned repository is a normal git repository. Edit files, commit as usual, then use
`josh push` to send your changes back upstream:

```shell
cd josh-docs
# ... edit files, git add, git commit ...
josh push
```

Josh transparently reverses the filter and applies your commits to the correct location
in the upstream repository. From the perspective of the rest of the team, the changes
appear exactly as if they had been pushed directly to the monorepo.

## Pulling changes

Use `josh pull` to fetch and integrate updates from upstream:

```shell
josh pull
```

## Cloning a part of a repository

Josh becomes particularly useful when you want to work on a filtered view of a larger
repository — for example, a single subdirectory or a composed workspace. The `josh` CLI
applies the filter client-side, which means the full repository object database is still
downloaded from the upstream host. The filter determines which commits and files are
visible in your working tree and which refs you can push to, but it does not reduce
transfer size.

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
