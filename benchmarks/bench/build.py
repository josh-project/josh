"""Building the josh binaries under test."""

import subprocess
from pathlib import Path

from bench.git import fetch_repo
from bench.shell import run


def build_josh_filter(commit: str, target_dir: str | Path) -> Path:
    """Fetch josh at `commit` and build josh-filter in release mode.

    Returns the path to the compiled `josh-filter` binary.
    """
    repo = fetch_repo(
        "https://github.com/josh-project/josh",
        "josh-bin",
        commit,
        target_dir,
    )
    run(
        "cargo build --bin josh-filter --release --target-dir=target",
        cwd=str(repo),
    )
    return Path(repo) / "target" / "release" / "josh-filter"


def _checked_out_at(repo: Path, commit: str) -> bool:
    """True if `repo` is a git checkout with HEAD at `commit`."""
    p = subprocess.run(
        ["git", "-C", str(repo), "rev-parse", "HEAD"],
        capture_output=True, text=True,
    )
    return p.returncode == 0 and p.stdout.strip() == commit


def build_josh_proxy(commit: str, target_dir: str | Path) -> dict[str, Path]:
    """Fetch josh at `commit`; build the binaries the sync scenario needs.

    Builds josh-proxy and the josh CLI (the subjects under test), josh-filter
    (reference filtered histories) and axum-cgi-server (serves the upstream
    mirror over HTTP, as josh-proxy only accepts http/ssh remotes). Returns
    binary name -> path.

    Unlike :func:`build_josh_filter` the checkout is NOT recreated when it is
    already at `commit`: full scenario runs take hours, and keeping the cargo
    target dir makes the rebuild incremental.
    """
    repo = Path(target_dir) / "josh-bin"
    if not _checked_out_at(repo, commit):
        repo = fetch_repo(
            "https://github.com/josh-project/josh",
            "josh-bin",
            commit,
            target_dir,
        )
    run(
        "cargo build --release --target-dir=target"
        " --bin josh-proxy --bin josh-filter --bin axum-cgi-server --bin josh",
        cwd=str(repo),
    )
    release = Path(repo) / "target" / "release"
    names = ("josh-proxy", "josh-filter", "axum-cgi-server", "josh")
    return {name: release / name for name in names}
