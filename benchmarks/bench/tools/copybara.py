"""copybara runner."""

from pathlib import Path

from bench.result import ToolResult
from bench.shell import run
from bench.timing import Timer


def run_copybara(source: Path, config_text: str, workdir: str | Path) -> ToolResult:
    """Run a copybara migration described by `config_text` in `workdir`.

    Unlike the in-tree tools, copybara migrates from an origin to a destination,
    reading the origin URL directly -- so there is no cold-start copy. We create
    a bare destination repo and interpolate both paths into the config.
    `config_text` must contain `{origin_url}` and `{dest_url}` placeholders. The
    result is left on refs/heads/main in the destination.
    """
    workdir = Path(workdir)
    workdir.mkdir(parents=True, exist_ok=True)

    dest_repo = workdir / "dest"
    run(f"git init --bare {dest_repo}")

    config_path = workdir / "copy.bara.sky"
    config_path.write_text(
        config_text.format(origin_url=source, dest_url=dest_repo)
    )

    with Timer() as t:
        run(
            f"copybara migrate {config_path} --init-history --force --ignore-noop",
            cwd=str(workdir),
        )
    return ToolResult("copybara", t.elapsed, dest_repo, "refs/heads/main")
