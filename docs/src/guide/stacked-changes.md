# Stacked Changes

Josh supports a stacked-changes workflow where a series of commits on a local branch
can each be pushed as a separate, independently-reviewable unit. This is useful when
working on a larger feature that is best reviewed in smaller, logical steps.

This feature is separate from Josh's filtering functionality. It works with any
repository accessible via the `josh` CLI, regardless of whether you are working with a
filtered view of a monorepo or a plain repository.

## Concepts

In a stacked changes workflow, each commit on your local branch represents one
self-contained change. When you use `josh changes publish`, Josh creates a
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

## Automatic dependency detection

When Josh publishes a change, it does not simply push the single commit. Instead, it
builds a minimal ref for that change: the change's commit, rebased onto the base branch,
together with only those intermediate commits from the stack that the change actually
depends on.

Josh determines dependencies by path intersection. It collects the set of files touched
by the change, then walks backwards through the intermediates: any commit whose touched
files overlap that set is included (and its files are added to the set, so transitive
dependencies are picked up too). Commits that touch entirely different files are omitted,
even if they sit between the base and the change in the local stack.

This means two independent changes that touch different parts of the codebase can each
be published directly on top of the base branch, even when they are interleaved on the
local branch. You do not need to carefully order your commits — Josh figures out which
changes need to come first.

**Example:** imagine a local stack with three commits:

```
[base] ← A (modifies auth/login.rs) ← B (modifies ui/button.rs) ← C (modifies auth/session.rs)
```

When publishing C, Josh sees it touches `auth/session.rs`. A also touches `auth/`, so A
is included. B only touches `ui/`, so B is skipped. C's ref will contain just A and C on
top of base. B's ref will contain only B on top of base.

## Ordering with `Change-Series:`

Sometimes two changes are logically ordered but touch disjoint files, so the automatic
path intersection does not pull them together. Assigning both the same `Change-Series:`
label tells Josh to treat them as ordered: when publishing the later commit, the earlier
one will be included even though they share no files.

```
Add unit tests for the new validation helper

Change: validation-tests
Change-Series: login-validation
```

```
Add input validation helper

Change: input-validation
Change-Series: login-validation
```

When Josh publishes `validation-tests`, it sees that both commits share the
`login-validation` series and ensures `input-validation` is published before it.

A commit can carry multiple `Change-Series:` footers if it belongs to more than one
ordered group.

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
josh changes publish
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
josh changes publish  # re-publish; existing PRs are updated, not recreated
```

As long as the change ID in the footer is preserved through your edits, `josh changes publish`
updates the correct existing PRs rather than creating new ones.

### 4. Merge

Once a PR is approved and its required checks pass, merge it through the forge's normal
UI. Then sync your local branch to account for the merged commit:

```shell
josh changes pull
```

This fast-forwards your branch to the updated upstream state, skipping local commits
whose `Change:` footer already appears upstream (i.e. merged changes) and restacking
your remaining changes on top. After pulling, the next `josh changes publish` will
retarget and promote the next PR in the stack from draft to ready for review.

## Publishing from a fork

If you cannot push to the target repository, point Josh at your own fork as a separate
push destination while still opening all pull requests against the target. Configure the
fork when adding the remote:

```shell
josh clone https://github.com/UPSTREAM/repo :/ work --push-url https://github.com/ME/repo
```

(or `josh remote add <name> <upstream-url> <filter> --push-url <fork-url>` for an existing
repository). `josh changes publish` then pushes the `@changes/…` refs to your fork, and
opens each pull request against the upstream repository with a cross-fork head
(`ME:@changes/…`).

Because a pull request's base branch must live in the repository the PR targets, fork
pull requests base on the upstream **default branch** rather than on an intermediate
`@base/…` branch. A change that still depends on unmerged predecessors is opened as a
**draft** — its diff temporarily includes those predecessors — and is promoted to ready
automatically once they merge and you re-publish.

## Publishing to Gerrit

Select the Gerrit forge when cloning (it cannot be auto-detected from the URL):

```shell
josh clone https://gerrit.example.com/repo :/ work --forge gerrit
```

`josh changes publish` then pushes to Gerrit's magic ref `refs/for/<branch>`, where the
push itself creates or updates the review. No `@changes/@base` refs are written and no
forge API is called, so there is nothing to `josh auth login`: authentication is handled
by git (your SSH key or HTTP credential helper).

Two publish modes are available, selected with `--gerrit-mode` and stored per-remote:

- **`independent`** (default) publishes only the changes that have no unmerged
  dependencies, each as its own separately-submittable review; dependent changes wait
  until their predecessors merge.
- **`stack`** publishes the whole history at once as a single Gerrit relation chain.

Josh generates the required `Change-Id: I<40 hex>` trailer for every pushed commit,
deterministically derived from the change's josh id — re-publishing therefore lands as a
new patchset on the same Gerrit change rather than a duplicate. See the
[forge reference](../reference/forge.md#gerrit) for details.

## Without forge integration

`josh changes publish` works without [forge integration](../reference/forge.md). Josh still
pushes the individual `@changes/…` refs to the upstream repository; you can then create
pull requests from them manually, or use them as part of a custom review workflow.
