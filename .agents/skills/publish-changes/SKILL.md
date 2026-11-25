---
name: publish-changes
description: Publish Josh review changes. Use whenever the user asks to publish, update, republish, or push a PR/change for review.
---

Follow the repository-level rules for one-commit PRs, commit messages, formatting, and publication.
This skill covers only the publication procedure.

## 1. Select the Josh command

When `josh` is available in the current environment, use it directly. Otherwise use
`cargo run --bin josh --`. In the commands below, `<josh>` means either invocation.

## 2. Identify the change

Inspect the current branch, commit, and associated PR/change using repository state and GitHub.
Do not ask the user for information available from `git`, `josh`, or `gh`.

Determine whether the change is new or already published, then identify the remote and refspec that
target that exact change and its current upstream branch.

## 3. Prepare the commit

Prepare the commit according to the root `AGENTS.md`. Stage only files intended for this change;
never stage unrelated work. Confirm the resulting review range contains exactly the intended
commit before publication.

## 4. Preview publication

Preview the exact operation:

```text
<josh> changes publish --dry-run [REMOTE] [REFSPECS...]
```

The preview must target the intended change/PR and upstream branch. Stop and correct ambiguous
remote selection, an unexpected destination, or an unintended stack before publishing.

## 5. Publish

Publish with the same Josh invocation, remote, and refspecs used by the successful preview, adding
any option required by the root `AGENTS.md`:

```text
<josh> changes publish [OPTIONS...] [REMOTE] [REFSPECS...]
```

## 6. Verify the published result

After publication:

1. Verify the forge PR/change head resolves to the amended commit SHA.
2. Verify the destination/base branch is unchanged unless the task intentionally changed it.
3. Verify the PR still contains one commit.
4. Check publication output and forge state for newly reported failures or review feedback.
5. Report the published commit SHA and PR/change URL.

Publication is incomplete until the forge points at the intended commit. A locally amended commit
or a ref parked only in the Josh store is not published.
