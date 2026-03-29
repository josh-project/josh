# Stacked Changes

Josh supports a stacked-changes workflow where a series of commits on a local branch
can each be pushed as a separate, independently-reviewable unit. This is useful when
working on a larger feature that is best reviewed in smaller, logical steps.

This feature is separate from Josh's filtering functionality. It works with any
repository accessible via the `josh` CLI, regardless of whether you are working with a
filtered view of a monorepo or a plain repository.

## Concepts

In a stacked changes workflow, each commit on your local branch represents one
self-contained change. When you use `josh publish`, Josh creates a
separate git ref for each qualifying commit.

A commit qualifies for a separate ref — and an automatic PR, when
[forge integration](../reference/forge.md) is configured — only if both of the
following are true:

1. **It has a change ID** in the commit message footer (see below).
2. **Its author email matches** the email configured in `user.email` in your git config.

Commits without a change ID, or authored by someone else, are silently skipped and are
not pushed as individual changes.

## Change IDs

A change ID is a short, stable identifier that you add manually to the footer of a
commit message, using either of these footers:

```
Change: my-feature-part-1
```

or the Gerrit-compatible form:

```
Change-Id: I1234abcd...
```

The change ID must not contain `@`. It must be unique within the stack. It is what
allows `josh push` to match a commit to an existing PR across rebases and amends —
so once you have assigned an ID to a change, keep it stable.

**Example commit message:**

```
Add input validation to the login form

Validates that the email field is non-empty and well-formed before
submission. Returns an error message inline without clearing the form.

Change: login-form-validation
```

## Workflow

### 1. Write your commits

Work on your feature normally, writing one commit per logical step. Add a `Change:`
footer to each commit you want to submit for review:

```
$ git commit -m "Add validation for input fields

Change: input-validation"

$ git commit -m "Wire validation into the form component

Change: form-wiring"

$ git commit -m "Add tests for form validation

Change: validation-tests"
```

Commits without a `Change:` footer are included in the push to the base branch but
do not get their own ref or PR.

### 2. Publish

```shell
josh publish
```

For each qualifying commit Josh pushes a ref under
`refs/heads/@changes/<base>/<author>/<change-id>`. With GitHub forge integration
enabled, a pull request is created (or updated) for each of these refs automatically.

The first change in the stack targets the repository's default branch. Each subsequent
PR targets the branch of the change before it. Intermediate PRs are automatically
marked as **draft** until the changes before them are merged.

### 3. Iterate

After receiving review feedback, amend or rebase your commits as needed, keeping the
`Change:` footers intact:

```shell
git rebase -i HEAD~3   # edit commits, preserve Change: footers
josh publish           # re-publish; existing PRs are updated, not recreated
```

As long as the change ID in the footer is preserved through your edits, `josh publish`
updates the correct existing PRs rather than creating new ones.

### 4. Merge

Once a PR is approved and its required checks pass, merge it through the forge's normal
UI. Then sync your local branch to account for the merged commit:

```shell
josh pull --rebase --autostash
```

This rebases your remaining local commits on top of the updated upstream state.
`--autostash` ensures any uncommitted changes are preserved across the operation. After
pulling, the next `josh publish` will retarget and promote the next PR in the stack
from draft to ready for review.

## Without forge integration

`josh publish` works without [forge integration](../reference/forge.md). Josh still
pushes the individual `@changes/…` refs to the upstream repository; you can then create
pull requests from them manually, or use them as part of a custom review workflow.
