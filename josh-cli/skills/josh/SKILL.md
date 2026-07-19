---
name: josh
description: Use Josh for filtered Git repositories, local workspaces, and structured automation.
---

# Josh

Use `josh` to work with reversible projections of Git repositories. Prefer small, explicit
commands and structured output. Do not parse human-facing text when JSON is available.

## Agent contract

- Run unattended commands as `josh --output json --quiet --non-interactive <command>`.
- JSON results always go to stdout, including failures. Always check the process exit status.
- Treat `data` as the command result. With `--quiet`, human `messages` are omitted.
- Schema version `1` is stable. Reject unknown major schema versions.
- Run `josh --output json --quiet capabilities --brief` once when capabilities are unknown.
- Use scoped help such as `josh workspace create --help`; do not load the full manual by default.

## Local workspace workflow

Create a definition in the current monorepo:

```sh
josh workspace create workspaces/frontend \
  --map app=:/apps/frontend \
  --map shared=:/libs/shared
```

Validate and materialize the current working tree, including uncommitted changes:

```sh
josh workspace validate workspaces/frontend
josh workspace checkout workspaces/frontend ../frontend-preview
```

The checkout is a detached preview. Build it with the project's normal build command. For an
editable checkout that can synchronize with an upstream repository, use:

```sh
josh clone <url> :workspace=workspaces/frontend <directory>
```

## Repository operations

Inspect state before mutation:

```sh
josh --output json --quiet status
josh --output json --quiet remote list
josh --output json --quiet remote show <name>
```

Use `josh fetch`, `josh pull`, and `josh push` instead of their Git equivalents for projected
repositories. Use `josh push --dry-run` before an unfamiliar push. Do not force-push unless the user
explicitly requests it.

## Filters

Quote filter expressions in the shell. Validate unfamiliar filters before using them:

```sh
josh --output json --quiet filter validate ':/services/backend'
josh --output json --quiet filter explain ':/services/backend:prefix=src'
```

A CLI workspace filter is `:workspace=<repository-relative-path>`. A full-repository projection is
`:/`.

## Failure handling

Read `error.code` first, then `error.message`, `error.causes`, and `error.hints`. Do not retry
`authentication.required`, `filter.invalid`, or `workspace.invalid` without changing inputs. Do not
silently add `--force`, discard changes, or overwrite an existing workspace or checkout.
