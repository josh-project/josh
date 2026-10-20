---
name: publish-changes
description: Commit, amend, and publish Josh review changes. Use whenever the user asks to publish, update, republish, or push a PR/change for review.
---

Josh review changes are published only through `josh changes publish`. This command owns the
stacked-change refs and forge integration. Never substitute `git push`, `josh push`, or
`gh pr create` for review publication.

## 1. Establish whether this is new or already published

Inspect the current branch, commit, and associated PR/change using repository state and GitHub.
Do not ask the user for information available from `git`, `josh`, or `gh`.

- An existing PR/change must remain one commit relative to its change base.
- A new review change gets one commit.
- Do not add a fixup or review-follow-up commit to an existing PR.

## 2. Prepare the single commit

Before committing, run the checks required by the change. Always run `cargo fmt` when Rust code
changed.

Stage only the files intended for this change; never stage unrelated work. For an existing
PR/change, amend its commit:

```text
git add <paths...>
git commit --amend --no-edit
```

If the message itself must change, amend it while preserving repository conventions:

- short subject beginning with an uppercase letter;
- no conventional-commit prefix;
- `Change: <alphanumeric-dash-id>` footer;
- `Assisted-By: <actual-model-id>` footer when an agent assisted;
- retain `Change-Series:` when the existing change uses it.

For a genuinely new change, create one commit with the same conventions instead of amending an
unrelated commit.

Confirm the worktree is clean and the resulting review range contains exactly the intended single
commit.

## 3. Preview publication

Use the current remote/refspec when already associated with a change. Otherwise derive them from
repository and forge state. Preview the exact operation:

```text
josh changes publish --dry-run [REMOTE] [REFSPECS...]
```

The preview must target the intended change/PR and upstream branch. Stop and correct ambiguous
remote selection, an unexpected destination, or an unintended stack before publishing.

## 4. Publish through Josh

Publish with the same remote and refspecs used by the successful preview:

```text
josh changes publish [REMOTE] [REFSPECS...]
```

When amending or rebasing an already published change requires a non-fast-forward update, use:

```text
josh changes publish --force [REMOTE] [REFSPECS...]
```

Use `--force` only for the intended existing change. Never bypass Josh with a direct force-push.

## 5. Verify the published result

After publication:

1. Verify the forge PR/change head resolves to the amended commit SHA.
2. Verify the destination/base branch is unchanged unless the task intentionally changed it.
3. Verify the PR still contains one commit.
4. Check publication output and forge state for newly reported failures or review feedback.
5. Report the published commit SHA and PR/change URL.

Publication is incomplete until the forge points at the intended commit. A locally amended commit
or a ref parked only in the Josh store is not published.
