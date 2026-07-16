"""Git repo fixtures and result verification."""

import os
import shutil
from pathlib import Path

from bench.result import ToolResult
from bench.shell import run


def fetch_repo(
    remote: str,
    name: str,
    revision: str,
    target_dir: str | Path,
) -> Path:
    """Fetch `revision` from `remote` into `target_dir/name`, returning its path.

    The destination is recreated from scratch each time so the checkout is
    reproducible.
    """
    repo_dir = Path(target_dir) / name

    if repo_dir.exists():
        shutil.rmtree(repo_dir)
    os.makedirs(repo_dir)

    run("git init", cwd=str(repo_dir))
    run(f"git fetch {remote} {revision}", cwd=str(repo_dir))
    run(f"git checkout {revision}", cwd=str(repo_dir))

    return repo_dir


def clone_pristine(src: str | Path, dest: str | Path) -> Path:
    """Copy `src` to `dest` with a pristine working tree, for a cold-start run.

    `dest` is recreated from scratch, so repeated runs start clean. The copy
    outlives this call (unlike a temp dir) so the result stays available for
    verification and inspection; the caller owns cleanup.
    """
    dest = Path(dest)
    if dest.exists():
        shutil.rmtree(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(src, dest)
    run("git reset --hard HEAD", cwd=str(dest))
    run("git clean -fdx", cwd=str(dest))
    return dest


def verify_result(result: ToolResult, expected_top_level: str) -> None:
    """Report a tool's result and check the filtered tree has the expected shape.

    Prints the elapsed time, the top-level entries, and the commit and file
    counts, then asserts the top-level tree matches `expected_top_level`.
    """
    repo, ref = str(result.repo), result.ref
    top = run(f"git ls-tree --name-only {ref}", cwd=repo).strip()
    n_commits = run(f"git rev-list --count {ref}", cwd=repo).strip()
    n_files = run(f"git ls-tree --name-only -r {ref} | wc -l", cwd=repo).strip()

    print(f"{result.name}: {result.elapsed:.3f}s")
    print(f"  top-level: {top}")
    print(f"  commits: {n_commits}, files: {n_files}")

    assert top == expected_top_level, (
        f"{result.name}: expected top-level {expected_top_level!r}, got {top!r}"
    )
