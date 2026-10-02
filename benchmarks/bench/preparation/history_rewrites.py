"""Prepare reproducible source histories for history-rewrite benchmarks."""

import os
from dataclasses import dataclass
from pathlib import Path

from bench.git import clone_pristine, fetch_repo
from bench.shell import run_args


@dataclass(frozen=True)
class RepositorySpec:
    """A source repository and paths shared by its benchmark filters."""

    name: str
    remote: str
    revision_env: str
    subtree: str
    suffix: str
    fast_tools_only: bool


@dataclass(frozen=True)
class PreparedRepository:
    """A local fixture spanning a cold base and a real upstream increment."""

    spec: RepositorySpec
    path: Path
    upstream_revision: str
    base: str
    tip: str
    range_commits: int


REPOSITORIES = {
    "josh": RepositorySpec(
        name="josh",
        remote="https://github.com/josh-project/josh.git",
        revision_env="HISTORY_REWRITE_JOSH_REV",
        subtree="josh-core",
        suffix=".rs",
        fast_tools_only=False,
    ),
    "git": RepositorySpec(
        name="git",
        remote="https://github.com/git/git.git",
        revision_env="HISTORY_REWRITE_GIT_REV",
        subtree="builtin",
        suffix=".c",
        fast_tools_only=True,
    ),
}

_COMMIT_ENV = {
    "GIT_AUTHOR_NAME": "Josh benchmark",
    "GIT_AUTHOR_EMAIL": "benchmark@josh-project.github.io",
    "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z",
    "GIT_COMMITTER_NAME": "Josh benchmark",
    "GIT_COMMITTER_EMAIL": "benchmark@josh-project.github.io",
    "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z",
}


def resolve_revision(spec: RepositorySpec) -> str:
    """Resolve the configured revision, defaulting to the remote's current HEAD."""
    configured = os.environ.get(spec.revision_env)
    if configured and len(configured) == 40 and all(
        character in "0123456789abcdefABCDEF" for character in configured
    ):
        return configured.lower()
    ref = configured or "HEAD"
    matches = run_args(["git", "ls-remote", spec.remote, ref]).split()
    if not matches:
        raise RuntimeError(f"{spec.remote}: revision {ref!r} was not found")
    return matches[0]


def prepare_repository(
    spec: RepositorySpec,
    target_dir: str | Path,
    *,
    increment_commits: int,
    smoke: bool = False,
) -> PreparedRepository:
    """Fetch ``spec`` with a base first-parent commits behind its tip."""
    target_dir = Path(target_dir)
    revision = resolve_revision(spec)
    source = fetch_repo(
        spec.remote,
        f"history-rewrites-source-{spec.name}",
        revision,
        target_dir,
    )
    fixture_name = (
        f"{spec.name}-{revision[:12]}-{increment_commits}"
        + ("-smoke" if smoke else "")
    )
    fixture = clone_pristine(
        source,
        target_dir / "history-rewrites-fixtures" / fixture_name,
    )

    raw_base = run_args(
        ["git", "rev-parse", f"{revision}~{increment_commits}"], cwd=fixture
    ).strip()
    range_commits = int(
        run_args(
            ["git", "rev-list", "--count", f"{raw_base}..{revision}"],
            cwd=fixture,
        )
    )
    base, tip = raw_base, revision
    if smoke:
        base_tree = run_args(
            ["git", "rev-parse", f"{raw_base}^{{tree}}"], cwd=fixture
        ).strip()
        tip_tree = run_args(
            ["git", "rev-parse", f"{revision}^{{tree}}"], cwd=fixture
        ).strip()
        base = run_args(
            ["git", "commit-tree", base_tree],
            cwd=fixture,
            env=_COMMIT_ENV,
            stdin=f"Snapshot of {spec.name} at {raw_base}\n",
        ).strip()
        tip = run_args(
            ["git", "commit-tree", tip_tree, "-p", base],
            cwd=fixture,
            env=_COMMIT_ENV,
            stdin=f"Snapshot of {spec.name} at {revision}\n",
        ).strip()

    run_args(["git", "branch", "-f", "base", base], cwd=fixture)
    run_args(["git", "checkout", "-B", "increment", tip], cwd=fixture)
    return PreparedRepository(
        spec, fixture, revision, base, tip, range_commits
    )
