"""Prepare the repos the rustc-josh-sync scenario replays against.

All of this is untimed preparation:

- an *upstream mirror* of rust-lang/rust served over HTTP behind josh-proxy,
  standing in for github.com/rust-lang/rust (and, for pushes, for the user's
  fork -- push branches are created directly on the mirror);
- per subtree, a persistent *filtered repo*: the rust history filtered with the
  production josh-sync filter. For josh-synced repos this IS the subtree repo's
  history (josh's roundtrip property), so it both serves as the mining substrate
  and stands in for the subtree GitHub repo -- no subtree clones needed;
- per subtree, a throwaway *work clone* of the filtered repo playing the role of
  the developer's subtree checkout during replays.
"""

import shutil
import subprocess
import time
from pathlib import Path

from bench.git import clone_pristine
from bench.preparation.rustc_josh_sync.config import JOSH_COMMIT, RUST_REVISION
from bench.shell import run


def remove_dir(path: Path) -> None:
    """Remove `path` recursively, tolerating transient APFS ENOTEMPTY races."""
    for _ in range(3):
        try:
            if path.exists():
                shutil.rmtree(path)
            return
        except OSError:
            time.sleep(0.2)
    run(f"rm -rf {path}")


def _ref_exists(repo: Path, ref: str) -> bool:
    """True if `ref` resolves in `repo`."""
    return (
        subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "--verify", "-q", ref],
            capture_output=True,
        ).returncode
        == 0
    )


def prepare_upstream(source: Path, work_dir: Path) -> tuple[Path, Path]:
    """Create the served upstream mirror; return (serve_root, repo_path).

    The mirror is recreated per run (replays mutate its branches). It lives at
    ``<serve_root>/rust-lang/rust.git`` so proxy URLs match the production
    shape. ``uploadpack.allowAnySHA1InWant`` lets josh-proxy fetch the mined
    historical (non-tip) commits addressed by ``@sha`` URLs.
    """
    serve_root = work_dir / "upstream"
    repo = serve_root / "rust-lang" / "rust.git"
    clone_pristine(source, repo, bare=True)
    run("git config http.receivepack true", cwd=str(repo))
    run("git config uploadpack.allowAnySHA1InWant true", cwd=str(repo))
    run("git symbolic-ref HEAD refs/heads/main", cwd=str(repo))
    return serve_root, repo


def prepare_filtered(
    josh_filter: Path,
    source: Path,
    name: str,
    filter_spec: str,
    target_dir: Path,
) -> Path:
    """Materialize the filtered (subtree-side) history for `name`; return the repo.

    Cached at ``target/rustc-josh-sync/filtered/<name>-<rust-sha>-<josh-sha>``,
    keyed by the rust pin and the josh version producing it (different josh
    versions may filter to different SHAs); on a hit (``refs/heads/filtered``
    exists) the repo is returned unchanged. josh-filter's persistent cache
    lives inside this repo, so later incremental filter runs during mining are
    cheap. This cache is entirely separate from josh-proxy's ``--local``
    cache: it never warms the proxy, so cold measurements stay cold.
    """
    repo = (
        target_dir
        / "rustc-josh-sync"
        / "filtered"
        / f"{name}-{RUST_REVISION[:12]}-{JOSH_COMMIT[:12]}"
    )
    if _ref_exists(repo, "refs/heads/filtered"):
        return repo

    clone_pristine(source, repo, bare=True)
    run(
        f"{josh_filter} -s '{filter_spec}' refs/heads/main --update refs/heads/filtered",
        cwd=str(repo),
    )
    return repo


def prepare_work_clone(filtered: Path, name: str, work_dir: Path) -> Path:
    """Create the developer-checkout stand-in for `name`; return its path.

    Recreated per run. ``--shared`` keeps it cheap and makes every historical
    subtree-side commit addressable in the clone (mined events reference them).
    """
    dest = work_dir / "work" / name
    remove_dir(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    run(f"git clone -q --shared --branch filtered {filtered} {dest}")
    run("git config user.name 'Josh Benchmark'", cwd=str(dest))
    run("git config user.email 'josh-bench@example.com'", cwd=str(dest))
    run("git config commit.gpgsign false", cwd=str(dest))
    return dest
