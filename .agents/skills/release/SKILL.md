---
name: release
description: Prepare and publish a Josh release. Use when the user asks to prepare, create, or publish a new release.
---

Josh releases are date-versioned GitHub releases. Publishing a GitHub release triggers
`.github/workflows/release.yml`, which builds and publishes multi-architecture `josh-proxy`
container images.

## Release identifiers

Choose the intended release date with the user if it is not explicit.

- Git tag and GitHub release: `rYY.MM.DD`, with zero-padded month and day.
- Cargo package version: `YY.M.D`, without leading zeroes because SemVer numeric components
  cannot contain them.
- Release notes: `releases/rYY.MM.DD.md`.

Never reuse or move an existing release tag. Check the latest Git tag and GitHub release before
choosing the identifier.

## 1. Establish the release range

1. Fetch the GitHub `master` branch and tags without overwriting local work.
2. Find the latest release tag with `git describe --tags --abbrev=0`.
3. Review first-parent commits from that tag through the intended release commit:
   `git log --first-parent --oneline <previous-tag>..HEAD`.
4. Recover associated PR numbers from GitHub when commit subjects do not contain them. Use the
   commit-to-pulls GitHub API rather than guessing from dates.
5. Confirm all changes intended for the release are merged to `master`. Do not release an
   unmerged topic branch or a dirty working tree.

## 2. Write curated release notes

Create `releases/<tag>.md`, following the latest files in `releases/`.

- Group entries first by component, such as `core`, `josh-cli`, `josh-proxy`, `josh-compose`,
  or `josh-gui`.
- Within a component, use only applicable headings: `New features`, `Breaking changes`,
  `Bug fixes`, and `Performance`.
- Curate user-visible behavior. Do not turn the commit log into release notes and do not list
  mechanical refactors, test-only changes, dependency churn, or CI maintenance unless operators
  must act on them.
- Put the behavior or outcome first, followed by the PR number in parentheses. Add a short
  explanation when the title alone does not communicate impact.
- Call out experimental features explicitly.
- For every breaking change, update `docs/src/guide/migration.md` with the affected version range
  and concrete migration steps. Keep the release note concise and point readers to the migration
  requirement as appropriate.

Do not publish or create the release tag yet. The notes and version bump must first be committed,
reviewed, and merged into the official `josh-project/josh` `master` branch.

## 3. Bump every release-versioned crate

Update the current release version to the new Cargo version in:

- every workspace package manifest whose package currently uses the shared date version;
- every versioned path dependency in the root `Cargo.toml`;
- the root `Cargo.lock` package entries.

Do not bump intentionally independent packages: `josh-gui` (`0.1.0`, `publish = false`),
`devtools/*` (`0.1.0`), or vendored crates. Do not update unrelated external dependencies while
regenerating `Cargo.lock`.

Verify the cutover with `cargo metadata --no-deps`: all date-versioned workspace packages and
all root path-dependency requirements must use the new version, and the old release version must
not remain in those locations.

Run `cargo fmt`. Put the release notes, any migration guide updates, and the complete version bump
in one release-preparation commit. The repository requires one commit per PR/change. Use a short
subject such as `Prepare <tag> release` and include both required footers:

```text
Assisted-By: <actual-model-id>
Change: prepare-release-rYY-MM-DD
```

The release-preparation commit must land in the official `josh-project/josh` `master` branch
before the release can proceed. After it merges, fetch `master` from a remote whose URL is the
official GitHub repository and verify that the preparation commit is its ancestor:

```text
git fetch <official-github-remote> master
git merge-base --is-ancestor <release-preparation-sha> FETCH_HEAD
```

Stop if this check fails. A local `master`, fork branch, proxy-tracking branch, open PR, or draft
change is not sufficient. The candidate must be a commit reachable from the fetched official
`master` and must contain the release-preparation commit.

## 4. Verify the release candidate

Before creating a tag or publishing a release:

1. Confirm the release-preparation commit and candidate SHA are both reachable from the freshly
   fetched official `josh-project/josh` `master`.
2. Confirm `cargo metadata --no-deps` reports the intended package versions.
3. Confirm `releases/<tag>.md` exists in the candidate commit and describes the full range since
   the previous release.
4. Record the exact candidate commit SHA. Use that SHA below; do not rely on a branch that can move.

Do not rerun `josh compose` or the integration suite for the release. Landing the preparation
commit in the official `master` branch is the CI gate; that commit already passed the required
checks before merge.

Do not run `cargo publish`. The normal Josh release publishes the GitHub release and container
images; crates.io publication is outside this workflow unless the user explicitly requests it.

## 5. Draft, review, and publish the GitHub release

Use `gh` against `josh-project/josh`. Create a draft whose tag, title, body, and target are exact:

```text
gh release create <tag> \
  --repo josh-project/josh \
  --target <candidate-sha> \
  --title <tag> \
  --notes-file releases/<tag>.md \
  --fail-on-no-commits \
  --draft
```

Inspect the draft through the GitHub API or `gh release view`. Verify the tag name, target SHA,
release title, complete notes body, and non-prerelease status. Publishing is the consequential
step: only publish when the user asked to publish, not merely to prepare a release.

Publish the reviewed draft with:

```text
gh release edit <tag> --repo josh-project/josh --draft=false
```

The `published` event triggers `.github/workflows/release.yml`. Do not separately create or move
the tag after publication.

## 6. Verify publication

A release is complete only after all of these checks pass:

1. The GitHub release is published, not a draft or prerelease, and resolves to the candidate SHA.
2. The `Build/publish images` workflow succeeded for the release event.
3. `ghcr.io/josh-project/josh-proxy:<tag>` and `:latest` are multi-architecture manifests with
   both `linux/amd64` and `linux/arm64` images.
4. If the Docker Hub secret was available to the workflow, `joshproject/josh-proxy:<tag>` and
   `:latest` resolve to the same release manifests. Docker Hub is optional in the workflow; GHCR
   is not.

If publication automation fails, diagnose and rerun the failed workflow against the same release.
Never retag a different commit or publish a second release to hide a failed build.
