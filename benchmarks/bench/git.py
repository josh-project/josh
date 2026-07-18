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
    *,
    bare: bool = False,
) -> Path:
    """Fetch `revision` from `remote` into `target_dir/name`, returning its path.

    The destination is recreated from scratch each time. When `bare` is set the
    result is a bare repository with `HEAD` pointing at `revision` via
    `refs/heads/main`, so object-level tools (josh-filter) that read `HEAD`
    resolve it without a checkout.
    """
    repo_dir = Path(target_dir) / name

    if repo_dir.exists():
        shutil.rmtree(repo_dir)
    os.makedirs(repo_dir)

    if bare:
        run("git init --bare", cwd=str(repo_dir))
        run(f"git fetch {remote} {revision}", cwd=str(repo_dir))
        # `git fetch <remote> <sha>` creates no ref and leaves HEAD dangling, so
        # point refs/heads/main -- and thus HEAD -- at the revision explicitly.
        run(f"git update-ref refs/heads/main {revision}", cwd=str(repo_dir))
        run("git symbolic-ref HEAD refs/heads/main", cwd=str(repo_dir))
    else:
        run("git init", cwd=str(repo_dir))
        run(f"git fetch {remote} {revision}", cwd=str(repo_dir))
        run(f"git checkout {revision}", cwd=str(repo_dir))

    return repo_dir


def clone_pristine(
    src: str | Path,
    dest: str | Path,
    *,
    bare: bool = False,
) -> Path:
    """Copy `src` to `dest` for a cold-start run; return `dest`.

    `dest` is recreated from scratch. The copy outlives this call (unlike a temp
    dir) so it stays available for verification and inspection; the caller owns
    cleanup.

    By default `dest` is a working-tree copy (`copytree` + reset/clean). When
    `bare` is set it is a bare clone (`git clone --bare`) for tools (josh-filter)
    that need no checkout; this also works when `src` is itself bare.
    """
    dest = Path(dest)
    if dest.exists():
        shutil.rmtree(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)

    if bare:
        run(f"git clone --bare {src} {dest}")
        return dest
    else:
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
