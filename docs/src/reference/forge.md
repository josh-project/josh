# Forge Integration

Forge integration is an **optional** feature that connects `josh` to a code hosting
platform (a "forge") such as GitHub. It is not required for normal git operations —
cloning, pushing, and pulling all work without it, even with private repositories.

Forge integration is specifically used for **automatic pull request management** during
[stacked changes](../guide/stacked-changes.md) workflows. When you push a stack of
commits with `josh push --split` or `josh push --stack`, `josh` can automatically
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

This opens a URL in the terminal and copies a one-time code to your clipboard. Open the
URL in your browser, enter the code, and authorize the application. The token is stored
securely in your system keyring.

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

Once authenticated, `josh push --split` or `josh push --stack` will, in addition to
pushing the git refs:

- **Create** a pull request for each commit that does not yet have one.
- **Update** existing pull requests (title, body, base branch) when commits are amended
  or rebased.
- **Manage draft status** automatically: pull requests whose base branch is not the
  repository's default branch are marked as drafts, and promoted to "ready for review"
  once they target the default branch directly.

See the [Stacked changes](../guide/stacked-changes.md) guide for a full walkthrough.

### Debug / verify authentication

```shell
josh auth debug github
```

This makes a test API call and prints the result, which is useful for verifying that
your token has the necessary permissions.
