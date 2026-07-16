"""josh-filter runner."""

from pathlib import Path

from bench.git import clone_pristine
from bench.result import ToolResult
from bench.shell import run
from bench.timing import Timer


def run_josh_filter(
    binary: Path,
    source: Path,
    spec: str,
    workdir: str | Path,
    ref: str = "refs/heads/filtered",
) -> ToolResult:
    """Run josh-filter with `spec` on a cold-start copy of `source` in `workdir`.

    `spec` is a josh filter expression, e.g.
    `:[josh/tests = :/tests, josh/docs = :/docs]`. The filtered history is
    written to `ref`.
    """
    repo = clone_pristine(source, Path(workdir) / "repo")
    with Timer() as t:
        run(f"{binary} -s '{spec}' HEAD --update {ref}", cwd=str(repo))
    return ToolResult("josh-filter", t.elapsed, repo, ref)
