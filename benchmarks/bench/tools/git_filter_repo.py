"""git-filter-repo runner."""

from pathlib import Path

from bench.git import clone_pristine
from bench.result import ToolResult
from bench.shell import run
from bench.timing import Timer


def run_git_filter_repo(source: Path, args: str, workdir: str | Path) -> ToolResult:
    """Run `git filter-repo <args>` on a cold-start copy of `source` in `workdir`.

    `args` is the native git-filter-repo argument string (paths, renames, etc.).
    The result is left on HEAD.
    """
    repo = clone_pristine(source, Path(workdir) / "repo")
    with Timer() as t:
        run(f"git filter-repo {args}", cwd=str(repo))
    return ToolResult("git-filter-repo", t.elapsed, repo, "HEAD")
