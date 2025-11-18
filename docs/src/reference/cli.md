# Command Line Tools

This page documents the command-line tools provided by Josh.

## josh

The `josh` CLI tool provides a convenient interface for working with Git repositories that use Josh filtering and projections. It wraps common Git operations with Josh-aware functionality, making it easier to clone, fetch, push, and manage filtered repositories.

### Installation

The `josh` CLI tool can be installed using Cargo:

```shell
cargo install josh-cli --git https://github.com/josh-project/josh.git
```

### Commands

#### `josh clone`

Clone a repository with optional projection/filtering.

**Usage:**
```shell
josh clone <URL> <FILTER> <OUT> [OPTIONS]
```

**Arguments:**
- `<URL>` - Remote repository URL
- `<FILTER>` - Workspace/projection identifier or path to spec (e.g., `:/docs`, `:/subdir`, or a filter specification)
- `<OUT>` - Checkout directory where the repository will be cloned

**Options:**
- `-b, --branch <BRANCH>` - Branch or ref to clone (default: `HEAD`)
- `--keep-trivial-merges` - Keep trivial merges (don't append `:prune=trivial-merge` to filters)
- `-h, --help` - Print help

**Examples:**
```shell
# Clone a subdirectory of a repository
josh clone https://github.com/example/repo.git :/docs ./docs-repo

# Clone with a specific branch
josh clone https://github.com/example/repo.git :/src ./src-repo -b main

# Clone with a custom filter
josh clone https://github.com/example/repo.git ":/subdir1::/subdir2" ./filtered-repo
```

#### `josh fetch`

Fetch from a remote (like `git fetch`) with projection-aware options. This command fetches unfiltered refs and then applies Josh filtering to create filtered references.

**Usage:**
```shell
josh fetch [OPTIONS]
```

**Options:**
- `-r, --remote <REMOTE>` - Remote name (or URL) to fetch from (default: `origin`)
- `-R, --ref <RREF>` - Ref to fetch (branch, tag, or commit-ish) (default: `HEAD`)
- `--prune` - Prune tracking refs no longer on the remote
- `-h, --help` - Print help

**Examples:**
```shell
# Fetch from the default remote (origin)
josh fetch

# Fetch from a specific remote
josh fetch -r upstream

# Fetch a specific branch
josh fetch -R main

# Fetch and prune deleted branches
josh fetch --prune
```

#### `josh pull`

Fetch & integrate from a remote (like `git pull`) with projection-aware options. This command combines `josh fetch` with `git pull` to integrate changes.

**Usage:**
```shell
josh pull [OPTIONS]
```

**Options:**
- `-r, --remote <REMOTE>` - Remote name (or URL) to pull from (default: `origin`)
- `-R, --ref <RREF>` - Ref to pull (branch, tag, or commit-ish) (default: `HEAD`)
- `--prune` - Prune tracking refs no longer on the remote
- `--ff-only` - Fast-forward only (fail if merge needed)
- `--rebase` - Rebase the current branch on top of the upstream branch
- `--autostash` - Automatically stash local changes before rebase
- `-h, --help` - Print help

**Examples:**
```shell
# Pull from the default remote
josh pull

# Pull with rebase
josh pull --rebase

# Pull with autostash (stash changes before rebase)
josh pull --rebase --autostash

# Fast-forward only
josh pull --ff-only
```

#### `josh push`

Push refs to a remote (like `git push`) with projection-aware options. This command applies reverse filtering before pushing to the remote.

**Usage:**
```shell
josh push [OPTIONS]
```

**Options:**
- `-r, --remote <REMOTE>` - Remote name (or URL) to push to (default: `origin`)
- `-R, --ref <REFSPECS>` - One or more refspecs to push (e.g., `main`, `HEAD:refs/heads/main`)
- `-f, --force` - Force update (non-fast-forward)
- `--atomic` - Atomic push (all-or-nothing if server supports it)
- `--dry-run` - Dry run (don't actually update remote)
- `--split` - Use split mode for pushing (defaults to normal mode)
- `--stack` - Use stack mode for pushing (defaults to normal mode)
- `-h, --help` - Print help

**Examples:**
```shell
# Push current branch to origin
josh push

# Push a specific branch
josh push -R main

# Force push
josh push -f

# Dry run to see what would be pushed
josh push --dry-run

# Atomic push (all refs succeed or none)
josh push --atomic
```

#### `josh remote`

Manage remotes with optional projection/filtering.

##### `josh remote add`

Add a remote with optional projection/filtering (like `git remote add`).

**Usage:**
```shell
josh remote add <NAME> <URL> <FILTER> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Remote name
- `<URL>` - Remote repository URL
- `<FILTER>` - Workspace/projection identifier or path to spec

**Options:**
- `--keep-trivial-merges` - Keep trivial merges (don't append `:prune=trivial-merge` to filters)
- `-h, --help` - Print help

**Examples:**
```shell
# Add a remote with a subdirectory filter
josh remote add upstream https://github.com/example/repo.git :/docs

# Add a remote with a custom filter
josh remote add filtered https://github.com/example/repo.git ":/subdir1::/subdir2"
```

#### `josh filter`

Apply filtering to existing refs (like `josh fetch` but without fetching). This command applies Josh filtering to refs that have already been fetched.

**Usage:**
```shell
josh filter <REMOTE>
```

**Arguments:**
- `<REMOTE>` - Remote name to apply filtering to

**Examples:**
```shell
# Apply filtering to the origin remote
josh filter origin

# Apply filtering to a specific remote
josh filter upstream
```

#### `josh link`

Manage josh links (like `josh remote` but for links). Links allow you to mount external repositories as subdirectories in your repository.

##### `josh link add`

Add a link with optional filter and target branch.

**Usage:**
```shell
josh link add <PATH> <URL> [FILTER] [OPTIONS]
```

**Arguments:**
- `<PATH>` - Path where the link will be mounted
- `<URL>` - Remote repository URL
- `<FILTER>` - Optional filter to apply to the linked repository

**Options:**
- `--target <TARGET>` - Target branch to link (defaults to `HEAD`)
- `-h, --help` - Print help

**Examples:**
```shell
# Add a link without filtering
josh link add vendor/example https://github.com/example/repo.git

# Add a link with a filter
josh link add vendor/example https://github.com/example/repo.git :/src

# Add a link with a specific target branch
josh link add vendor/example https://github.com/example/repo.git --target main
```

##### `josh link fetch`

Fetch from existing link files.

**Usage:**
```shell
josh link fetch [PATH]
```

**Arguments:**
- `<PATH>` - Optional path to specific `.josh-link.toml` file (if not provided, fetches all)

**Examples:**
```shell
# Fetch all links
josh link fetch

# Fetch a specific link
josh link fetch vendor/example
```

### How It Works

The `josh` CLI tool works by:

1. **Storing filter information**: When you add a remote with `josh remote add`, the filter is stored in Git config under `josh-remote.<name>.filter`.

2. **Fetching unfiltered refs**: When fetching, the tool first fetches unfiltered refs to `refs/josh/remotes/<remote>/<branch>`.

3. **Applying filters**: The tool then applies the configured filter to create filtered refs in `refs/namespaces/josh-<remote>/refs/heads/<branch>`.

4. **Reverse filtering on push**: When pushing, the tool applies reverse filtering to convert filtered commits back to the original repository structure before pushing to the remote.

### Configuration

Josh remotes are configured in Git's config using the following keys:
- `josh-remote.<name>.url` - The remote repository URL
- `josh-remote.<name>.filter` - The filter specification
- `josh-remote.<name>.fetch` - The refspec for fetching

These are automatically set when using `josh remote add` or `josh clone`.

---

## josh-filter

Command to rewrite history using ``josh`` filter specs.
By default it will use ``HEAD`` as input and update ``FILTERED_HEAD`` with the filtered
history, taking a filter specification as argument.
(Note that input and output are swapped with `--reverse`.)

It can be installed with the following Cargo command, assuming Rust is installed:
```shell
cargo install josh-filter --git https://github.com/josh-project/josh.git
```

---

## git-sync

A utility to make working with server side rewritten commits easier.
Those commits frequently get produced when making changes to ``workspace.josh`` files.

The command is available [in the script
directory](https://raw.githubusercontent.com/josh-project/josh/master/scripts/git-sync).
It should be put downloaded and added to the ``PATH``.
It can then be used as a drop-in replacement for ``git push``.
It enables the server to *return* commits back to the client after a push. This is done by parsing
the messages sent back by the server for announcements of rewritten commits and then fetching
those to update the local references.
In case of a normal git server that does not rewrite anything, ``git sync`` will do exactly the
same as ``git push``, also accepting the same arguments.
