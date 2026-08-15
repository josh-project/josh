"""git filter-branch runner."""

from pathlib import Path

from bench.git import clone_pristine
from bench.result import ToolResult
from bench.shell import run
from bench.timing import Timer


def run_git_filter_branch(
    source: Path,
    tree_filter_script: str,
    workdir: str | Path,
) -> ToolResult:
    """Run `git filter-branch --tree-filter` on a cold-start copy of `source`.

    `tree_filter_script` is a shell script run in the working tree of every
    commit. --tree-filter checks out each commit and runs the script, so this is
    O(n_commits * n_files) and notoriously slow -- see the git docs' PERFORMANCE
    and SAFETY warnings. The result is left on HEAD.
    """
    repo = clone_pristine(source, Path(workdir) / "repo")

    # Write the tree-filter script to a file to avoid shell quoting issues.
    script_path = repo / "_tree_filter.sh"
    script_path.write_text(tree_filter_script)

    with Timer() as t:
        run(
            "FILTER_BRANCH_SQUELCH_WARNING=1 "
            "git filter-branch --force --prune-empty "
            f"--tree-filter 'sh {script_path}' HEAD",
            cwd=str(repo),
        )
    return ToolResult("git filter-branch", t.elapsed, repo, "HEAD")
