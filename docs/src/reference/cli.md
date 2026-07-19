# josh CLI

The `josh` command is the primary tool for working with filtered git repositories.
It provides projection-aware equivalents of the common git operations.

## Installation

```shell
cargo install josh-cli --locked --git https://github.com/josh-project/josh.git
```

## Global options

Global options may appear before or after a subcommand.

| Flag | Description |
|------|-------------|
| `--output <human|json|jsonl>` | Select human or versioned machine output |
| `--pretty` | Pretty-print `--output json` instead of emitting one compact line |
| `-q`, `--quiet` | Suppress diagnostics and omit machine-output messages |
| `--color <auto|always|never>` | Control color in Josh and child Git processes |
| `--no-progress` | Disable progress output and capture child process output |
| `--non-interactive` | Disable prompts, browser launches, and credential input |

`JOSH_OUTPUT`, `JOSH_COLOR`, and `JOSH_NON_INTERACTIVE` provide environment-based defaults.
Machine output is described in [Automation and agent use](#automation-and-agent-use).

---

## josh clone

Clone a repository, optionally applying a filter projection.

```
josh clone <url> <filter> <out> [options]
```

| Argument | Description |
|----------|-------------|
| `<url>`  | Remote repository URL (HTTPS, SSH, or local path) |
| `<filter>` | [Filter spec](./filters.md), such as `:/docs` or `:workspace=workspaces/app` |
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
| `-R`, `--ref <branch>` | Fetch one branch; `HEAD` fetches the configured set (default) |

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
| `-R`, `--ref <branch>` | Pull one branch; `HEAD` uses the configured upstream (default) |
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

## josh changes publish

Push each commit as an independent, minimal diff (stacked changes workflow). Each commit
with a [Change ID](../guide/stacked-changes.md) is pushed to its own ref and, when
[forge integration](./forge.md) is configured, gets its own pull request.

```
josh changes publish [<remote>] [<refspecs>...] [options]
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

### josh changes list

List locally synchronized changes and their commits and comments.

```shell
josh changes list [--branch <branch>]
```

### josh changes comment

Store a local comment or queue it for a remote forge.

```shell
josh changes comment <change> --message <text> [options]
```

Important options include `--file`, `--location <path:line>`, `--reply-to`, `--update-of`,
`--branch`, and `--remote`.

### josh changes sync

Synchronize changes and review comments with the configured forge.

```shell
josh changes sync [remote] [--clean] [--local] [--push]
```

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

### josh remote list

List every configured Josh remote in deterministic name order.

```shell
josh remote list
```

### josh remote show

Show the URL, filter, fetch refspec, and forge integration for a remote.

```shell
josh remote show <name>
```

### josh remote set-filter

Replace a remote's projection filter. The new filter must be reversible. Add `--apply` to update
already fetched refs immediately; otherwise the next `josh fetch` applies it.

```shell
josh remote set-filter <name> <filter> [--apply]
```

### josh remote remove

Remove a Josh remote, its configuration, and its private tracking refs.

```shell
josh remote remove <name> [--dry-run]
```

---

## josh status

Show the current branch, working-tree state, and configured Josh remotes.

```shell
josh status
josh --output json status
```

---

## josh agent skill

Josh includes a concise, Agent Skills-compatible `SKILL.md`. Print it for inspection or install it
into the directory used by an agent:

```shell
josh agent skill print
josh agent skill install
josh agent skill install --target .claude/skills/josh
josh agent skill install --target /custom/agent/skills/josh
```

The default installation directory is `.agents/skills/josh`. Installation creates
`<target>/SKILL.md`, refuses to overwrite an existing file, and supports `--force` and `--dry-run`.
Machine output reports the installed path and skill version.

---

## josh workspace

Create and inspect versioned projection workspaces. A workspace is represented by a
`workspace.josh` file, so every operation remains compatible with manual file editing.

### josh workspace create

Create a workspace definition in the current repository. Each `--map` has the form
`DESTINATION=FILTER` and may be repeated.

```shell
josh workspace create workspaces/frontend \
  --map app=:/apps/frontend \
  --map libs/shared=:/libs/shared
```

Use `--dry-run` to validate and display the definition without writing it. Existing definitions
are protected unless `--force` is supplied.

### josh workspace list and show

```shell
josh workspace list
josh workspace list --verbose
josh workspace show workspaces/frontend
```

`list` includes invalid definitions instead of silently skipping them.

### josh workspace validate

Validate syntax and reversibility. With no paths, every workspace in the working tree is checked.

```shell
josh workspace validate
josh workspace validate workspaces/frontend workspaces/backend
```

The command exits unsuccessfully when any requested definition is invalid.

### josh workspace checkout

Materialize the workspace as a detached local Git worktree. The default input is `.` and therefore
includes current working-tree changes, which makes this useful for previewing a new definition.

```shell
josh workspace checkout workspaces/frontend ../frontend-preview
josh workspace checkout workspaces/frontend ../frontend-at-head --reference HEAD
```

This checkout is a local preview. For a bidirectional remote checkout, use `josh clone` with the
`:workspace=<path>` filter.

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

Validate or explain a filter without entering a repository:

```shell
josh filter validate ':/services/backend'
josh filter explain ':/services/backend:prefix=src'
josh --output json filter explain ':/services/backend'
```

Validation requires the filter to be reversible so it is safe for bidirectional CLI workflows.

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

## josh link

Manage versioned links to other repositories.

```shell
josh link add <path> <url> [filter] [--target <branch>] [--mode <mode>]
josh link fetch [filter]
josh link update [filter]
josh link push <path> [--force]
```

Link modes are `embedded`, `snapshot`, and `pointer`; the default is `snapshot`. Link operations
are currently experimental and require `JOSH_EXPERIMENTAL_FEATURES=1` where noted by the command.

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

## josh compose run

> **Experimental:** requires `JOSH_EXPERIMENTAL_FEATURES=1`.

Run a workspace in an isolated, automatically-cached container. See
[josh compose run](../contributing/josh-run.md) for full documentation.

```
josh compose run [OPTIONS] [REFERENCE] [FILTER]
```

| Argument | Description |
|----------|-------------|
| `[REFERENCE]` | Input: `.`, `+`, `HEAD`, or any Git ref (default: `.`) |
| `[FILTER]` | Workspace filter (default: `:+compose`, using `compose.josh`) |

**Options:**

| Flag | Description |
|------|-------------|
| `--clean` | Remove cached images and output volumes |
| `--clean-all` | Remove cached images, output volumes, and persistent cache volumes |

**Examples:**

```shell
# Run the default workspace defined in compose.josh
josh compose run

# Run the test workspace against the working tree
josh compose run . :+ws/test

# Run using only staged changes
josh compose run + :+ws/test
```

---

## Automation and agent use

Use `--output json` for one versioned result document or `--output jsonl` for message events
followed by a final result. Machine-readable results are always written to stdout, including
failures. Human diagnostics use stderr, and failures also use a non-zero exit status.

Every result contains:

- `schema_version` — currently `"1"`;
- `type` — `"result"`, `"message"`, or `"help"`;
- `command` — a stable dotted command name such as `workspace.list`;
- `success` — whether the command completed successfully;
- `data` — command-specific structured data;
- `messages` — captured human diagnostics, omitted when `--quiet` is used;
- `error.code`, `error.message`, `error.causes`, and `error.hints` on failure.

Stable error codes currently include:

| Code | Meaning |
|------|---------|
| `cli.usage` | Command-line syntax or argument error |
| `repository.not_found` | No Git repository was found |
| `authentication.required` | Forge credentials are unavailable |
| `remote.not_found` | A configured Josh remote was not found |
| `reference.not_found` | A requested Git reference was not found |
| `filter.invalid` | A filter is malformed or not reversible |
| `workspace.not_found` | A workspace definition does not exist |
| `workspace.already_exists` | Creation would overwrite a workspace |
| `workspace.destination_exists` | A checkout destination already exists |
| `workspace.invalid` | A workspace is malformed or cannot be materialized |
| `clone.destination_exists` | A clone destination is not empty |
| `agent_skill.already_exists` | Skill installation would overwrite an existing file |
| `git.command_failed` | A child Git command failed |
| `josh.command_failed` | An uncategorized command failure |

Machine modes imply non-interactive behavior and capture child Git output. For unattended use,
being explicit is still recommended:

```shell
josh --output json --quiet --non-interactive --no-progress workspace list
```

JSON is compact by default. Add `--pretty` for interactive inspection. `--quiet` produces the most
context-efficient form: the versioned envelope and typed `data`, without duplicated human messages.

Discover supported formats and features without entering a repository. Agents should prefer the
brief form during capability negotiation:

```shell
josh capabilities
josh --output json --quiet capabilities --brief
```

Generate native completions for Bash, Zsh, Fish, Elvish, or PowerShell:

```shell
josh completions bash > ~/.local/share/bash-completion/completions/josh
josh completions zsh > ~/.zfunc/_josh
```

Consumers must reject unknown major schema versions rather than guessing their meaning. Fields may
be added within a schema version, but existing fields will not change meaning.

---

## josh-filter (standalone binary)

`josh-filter` is a lower-level command that rewrites git history using Josh filter specs.
It is intended for scripting and one-off history rewriting tasks rather than day-to-day
development workflows.

**Input:** the second positional argument selects what to filter. It defaults to `HEAD`
but can be any of:

- `.` - the working tree (including uncommitted changes)
- `+` - the index (staged changes only)
- A full or abbreviated commit SHA
- A ref name (e.g. `main`, `refs/heads/feature`)

**Output:** the filtered commit SHA is printed to stdout. The filtered history is also
written to the ref given by `--update` (default: `FILTERED_HEAD`).

**Basic usage:**

```shell
# Filter HEAD through :/docs and write result to FILTERED_HEAD
josh-filter :/docs

# Filter the working tree and print the resulting SHA
josh-filter :/docs .

# Filter a specific commit SHA
josh-filter :/docs abc1234 --update refs/my/filtered
```

**Options:**

| Flag | Description |
|------|-------------|
| `--update <ref>` | Ref to update with the filtered result (default: `FILTERED_HEAD`) |
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
| `--reverse` | Reverse-apply the filter to reconstruct upstream commits |
| `--check-roundtrip` | Verify reverse application; exits 1 when the round trip differs |
