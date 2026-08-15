"""Building the josh-filter binary under test."""

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
