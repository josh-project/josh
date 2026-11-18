"""Building the josh binaries under test."""

import json
import subprocess
import urllib.request
from pathlib import Path

from bench.git import fetch_repo
from bench.shell import run

JOSH_REMOTE = "https://github.com/josh-project/josh"
GITHUB_LATEST_RELEASE = (
    "https://api.github.com/repos/josh-project/josh/releases/latest"
)


def latest_josh_release() -> tuple[str, str]:
    """Return the latest published release tag and its peeled commit SHA."""
    request = urllib.request.Request(
        GITHUB_LATEST_RELEASE,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "josh-history-rewrite-benchmark",
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        tag = json.load(response)["tag_name"]

    refs = subprocess.run(
        [
            "git",
            "ls-remote",
            JOSH_REMOTE,
            f"refs/tags/{tag}",
            f"refs/tags/{tag}^{{}}",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    if not refs:
        raise RuntimeError(f"latest Josh release tag {tag!r} was not found")

    peeled = [line for line in refs if line.endswith("^{}")]
    commit = (peeled or refs)[-1].split()[0]
    return tag, commit


def _build_josh(commit: str, target_dir: str | Path, bins: tuple[str, ...]) -> Path:
    """Fetch josh at `commit`, build `bins` in release mode; return the bin dir.

    `fetch_repo` leaves an already-fetched checkout alone, so the cargo target
    dir inside it survives across runs and rebuilds are incremental.
    """
    repo = fetch_repo(JOSH_REMOTE, "josh-bin", commit, target_dir)
    selected = " ".join(f"--bin {name}" for name in bins)
    run(f"cargo build --release --target-dir=target {selected}", cwd=str(repo))
    return Path(repo) / "target" / "release"


def build_josh_filter(commit: str, target_dir: str | Path) -> Path:
    """Fetch josh at `commit` and build josh-filter in release mode.

    Returns the path to the compiled `josh-filter` binary.
    """
    return _build_josh(commit, target_dir, ("josh-filter",)) / "josh-filter"


def build_latest_josh_filter(target_dir: str | Path) -> tuple[Path, str, str]:
    """Build josh-filter from the latest published Josh release."""
    tag, commit = latest_josh_release()
    return build_josh_filter(commit, target_dir), tag, commit


def build_josh_proxy(commit: str, target_dir: str | Path) -> dict[str, Path]:
    """Fetch josh at `commit`; build the binaries the sync scenario needs.

    Builds josh-proxy and the josh CLI (the subjects under test), josh-filter
    (reference filtered histories) and axum-cgi-server (serves the upstream
    mirror over HTTP, as josh-proxy only accepts http/ssh remotes). Returns
    binary name -> path.
    """
    names = ("josh-proxy", "josh-filter", "axum-cgi-server", "josh")
    release = _build_josh(commit, target_dir, names)
    return {name: release / name for name in names}
