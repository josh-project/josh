# Forge Integration

Forge integration is an **optional** feature that connects `josh` to a code hosting
platform (a "forge") such as GitHub. It is not required for normal git operations —
cloning, pushing, and pulling all work without it, even with private repositories.

Forge integration is specifically used for **automatic pull request management** during
[stacked changes](../guide/stacked-changes.md) workflows. When you push a stack of
commits with `josh changes publish`, `josh` can automatically
create or update one pull request per commit on the forge.

## GitHub

GitHub is currently the only supported forge.

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
