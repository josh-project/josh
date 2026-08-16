# Forge Integration

Forge integration is an **optional** feature that connects `josh` to a code hosting
platform (a "forge") such as GitHub or Gerrit. It is not required for normal git
operations — cloning, pushing, and pulling all work without it, even with private
repositories.

Forge integration shapes how `josh changes publish` turns a stack of commits into
reviews. On GitHub it manages one pull request per commit; on Gerrit it pushes each
change to the server's magic `refs/for/<branch>` ref, where the push itself creates or
updates the review.

The forge is chosen per remote with `--forge <github|gerrit>` on `josh clone` /
`josh remote add`. GitHub is auto-detected from the URL; Gerrit is not identifiable from
a URL and must be selected explicitly. It is stored as a `forge` meta key in the remote
config file (`<git-common-dir>/josh/remotes/<name>.josh`).

## GitHub

### Authentication

`josh` uses GitHub's [device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow)
for authentication — the same flow used by the official GitHub CLI.

**Log in:**

```shell
josh auth login github
```

This prints a URL and a one-time code. If clipboard access is available the code is
also copied automatically, otherwise it is only printed to the terminal. Open the URL
in your browser, enter the code, and authorize the application.

The token is stored in `~/.config/josh-cli/credentials.json` with `0600` permissions.

**Log out:**

```shell
josh auth logout github
```

**Alternatively**, set the `GH_TOKEN` environment variable to a
[personal access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens).
This takes precedence over any stored token and is useful in CI environments:

```shell
export GH_TOKEN=ghp_...
```

### What forge integration enables

Once authenticated, `josh changes publish` will, in addition to pushing the git refs:

- **Create** a pull request for each commit that does not yet have one.
- **Update** existing pull requests (title, body, base branch) when commits are amended
  or rebased.
- **Manage draft status** automatically: pull requests whose base branch is not the
  repository's default branch are marked as drafts, and promoted to "ready for review"
  once they target the default branch directly.

See the [Stacked changes](../guide/stacked-changes.md) guide for a full walkthrough.

### Publishing from a fork

When you do not have push access to the target repository, configure a separate **push
URL** (your fork) with `--push-url` on `josh remote add` (or `josh clone`):

```shell
josh clone https://github.com/UPSTREAM/repo :/ work --push-url https://github.com/ME/repo
```

Change branches are then pushed to your fork while the pull requests — including upstack
drafts — are opened against the upstream repository with a cross-fork head
(`ME:@changes/…`). The push URL is stored as a `push` meta key in the remote config file
(`<git-common-dir>/josh/remotes/<name>.josh`), analogous to git's
`remote.<name>.pushurl`.

Because a GitHub pull request's base branch must live in the repository the PR is opened
against, fork PRs always target the upstream **default branch**. A change that still
depends on unmerged predecessors is opened as a **draft** (its diff temporarily includes
those dependencies) and is automatically promoted to "ready for review" once they merge
and you re-publish.

## Gerrit

Gerrit is selected explicitly, since a Gerrit server cannot be recognized from its URL:

```shell
josh clone https://gerrit.example.com/repo :/ work --forge gerrit
```

### Authentication

Gerrit publishing is a plain `git push` to the server's magic `refs/for/<branch>` ref,
so `josh` manages **no** Gerrit credentials — authentication is handled by git itself
(an SSH key, or an HTTP credential helper with your Gerrit HTTP password). There is
nothing to `josh auth login`.

### What forge integration enables

With the Gerrit forge selected, `josh changes publish` pushes to `refs/for/<branch>`
instead of GitHub's `@changes`/`@base` ref pairs. No API call is made — the push itself
creates or updates the reviews.

### Publish modes

Because Gerrit keys a change by its `Change-Id` and expects it to appear exactly once
with a single parent, josh's `downstack` model (where one change can belong to several
stacks with differing ancestry) cannot be mapped onto Gerrit one-to-one. Two modes
resolve this, selected per remote with `--gerrit-mode` on `josh clone` /
`josh remote add` and stored as a `gerrit-mode` meta key:

- **`independent`** (default) — push only the changes that have **no dependencies**:
  those sitting directly on the target base. Each becomes its own independent, separately
  submittable review (one `git push` per change). A change that still depends on unmerged
  work is skipped until its dependencies merge and it becomes a root itself. Because only
  roots are pushed, no change is ever duplicated across stacks.
- **`stack`** — push the entire commit history once as a single Gerrit **relation
  chain**: one change per commit, each with exactly one parent. This publishes the whole
  stack at once, at the cost of Gerrit ordering the changes (they submit bottom-up).

In both modes, Gerrit requires every pushed commit to carry a `Change-Id: I<40 hex>`
trailer. `josh` generates one automatically, derived deterministically from the change's
josh id (and reused as-is if the commit already carries a valid Gerrit Change-Id). The
deterministic derivation is what makes re-publishing land as a **new patchset on the same
Gerrit change** rather than creating a duplicate. The trailer is written only onto the
commits that are pushed; your local history is left untouched.
