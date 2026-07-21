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
    *,
    input_ref: str = "HEAD",
    bare: bool = False,
) -> ToolResult:
    """Run josh-filter with `spec` on a cold-start copy of `source` in `workdir`.

    `spec` is a josh filter expression, e.g.
    `:[josh/tests = :/tests, josh/docs = :/docs]`. The filter runs against
    `input_ref` and the filtered history is written to `ref`.

    The pristine copy (untimed) makes each run a cold start; only the filter
    call is measured. Pass `bare=True` for object-level filtering with no
    working-tree checkout.
    """
    repo = clone_pristine(source, Path(workdir) / "repo", bare=bare)
    with Timer() as t:
        run(f"{binary} -s '{spec}' {input_ref} --update {ref}", cwd=str(repo))
    return ToolResult("josh-filter", t.elapsed, repo, ref)
