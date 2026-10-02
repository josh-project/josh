"""Native cold and incremental history-rewrite benchmark runners."""

import shlex
import shutil
import statistics
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from bench.git import clone_pristine
from bench.preparation.history_rewrites import PreparedRepository, RepositorySpec
from bench.shell import run_args
from bench.timing import Timer

PREFIX = "project"
FAST_TOOLS = ("josh", "git-filter-repo")
ALL_TOOLS = FAST_TOOLS + ("git-filter-branch", "copybara")


@dataclass(frozen=True)
class Transformation:
    """Equivalent native expressions for one tree transformation."""

    name: str
    josh: str
    filter_repo: tuple[str, ...]
    copybara_files: str
    copybara_transformations: tuple[str, ...]


@dataclass(frozen=True)
class RewriteSample:
    """One verified timed phase of a tool run."""

    repository: str
    transformation: str
    tool: str
    phase: str
    iteration: int
    elapsed: float
    files: int


def transformations(source: PreparedRepository) -> dict[str, Transformation]:
    """Return the three equivalent transformations for ``source``."""
    subtree = source.spec.subtree
    suffix = source.spec.suffix
    return {
        "subtree": Transformation(
            "subtree",
            f":/{subtree}",
            ("--subdirectory-filter", subtree),
            f'glob(["{subtree}/**"])',
            (f'core.move("{subtree}", "")',),
        ),
        "prefix": Transformation(
            "prefix",
            f":prefix={PREFIX}",
            ("--to-subdirectory-filter", PREFIX),
            'glob(["**"])',
            (f'core.move("", "{PREFIX}")',),
        ),
        "glob": Transformation(
            "glob",
            f"::**/*{suffix}",
            ("--path-glob", f"*{suffix}"),
            f'glob(["**{suffix}"])',
            (),
        ),
    }


def preflight_tool(tool: str, binary: Path, workdir: Path) -> None:
    """Exercise a tiny rewrite outside the benchmark timer."""
    if workdir.exists():
        shutil.rmtree(workdir)
    source_path = workdir / "source"
    source_path.mkdir(parents=True)
    run_args(["git", "init"], cwd=source_path)
    run_args(["git", "config", "user.name", "Josh benchmark"], cwd=source_path)
    run_args(
        ["git", "config", "user.email", "benchmark@josh-project.github.io"],
        cwd=source_path,
    )
    selected = source_path / "selected"
    selected.mkdir()
    marker = selected / "file.rs"
    marker.write_text("cold\n")
    run_args(["git", "add", "."], cwd=source_path)
    run_args(
        ["git", "-c", "commit.gpgsign=false", "commit", "-m", "Cold"],
        cwd=source_path,
    )
    base = run_args(["git", "rev-parse", "HEAD"], cwd=source_path).strip()
    marker.write_text("incremental\n")
    run_args(
        ["git", "-c", "commit.gpgsign=false", "commit", "-am", "Incremental"],
        cwd=source_path,
    )
    tip = run_args(["git", "rev-parse", "HEAD"], cwd=source_path).strip()
    spec = RepositorySpec(
        name="preflight",
        remote="",
        revision_env="",
        subtree="selected",
        suffix=".rs",
        fast_tools_only=False,
    )
    source = PreparedRepository(spec, source_path, tip, base, tip, 1)
    run_pair(
        tool,
        binary,
        source,
        transformations(source)["subtree"],
        workdir / "run",
        0,
    )


def run_pair(
    tool: str,
    binary: Path,
    source: PreparedRepository,
    transform: Transformation,
    workdir: Path,
    iteration: int,
) -> list[RewriteSample]:
    """Run and verify a cold rewrite followed by a catch-up update."""
    runners = {
        "josh": _run_josh,
        "git-filter-repo": _run_filter_repo,
        "git-filter-branch": _run_filter_branch,
        "copybara": _run_copybara,
    }
    return runners[tool](binary, source, transform, workdir, iteration)


def median_rows(samples: list[RewriteSample]) -> list[dict[str, object]]:
    """Aggregate samples for reporting and charting."""
    keys = dict.fromkeys(
        (s.repository, s.transformation, s.phase, s.tool) for s in samples
    )
    rows = []
    for repository, transformation, phase, tool in keys:
        values = [
            s.elapsed
            for s in samples
            if (
                s.repository,
                s.transformation,
                s.phase,
                s.tool,
            )
            == (repository, transformation, phase, tool)
        ]
        rows.append(
            {
                "repository": repository,
                "transformation": transformation,
                "phase": phase,
                "tool": tool,
                "elapsed_s": statistics.median(values),
                "samples": len(values),
            }
        )
    return rows


def _run_josh(binary, source, transform, workdir, iteration):
    repo = clone_pristine(source.path, workdir / "repo", bare=True)
    run_args(["git", "update-ref", "refs/heads/source", source.base], cwd=repo)
    _warm_git_graph(repo, "refs/heads/source")
    command = [
        binary,
        "-s",
        transform.josh,
        "refs/heads/source",
        "--update",
        "refs/heads/output",
    ]

    cold = _timed(command, repo)
    cold_files = _verify(source, source.base, transform, repo, "refs/heads/output")
    _remember_cold(repo, "refs/heads/output")
    run_args(["git", "update-ref", "refs/heads/source", source.tip], cwd=repo)
    _warm_git_graph(repo, f"{source.base}..{source.tip}")
    incremental = _timed(command, repo)
    incremental_files = _verify(
        source, source.tip, transform, repo, "refs/heads/output"
    )
    _verify_incremental(repo, "refs/heads/output")
    return _samples(
        source, transform, "josh", iteration, cold, incremental,
        cold_files, incremental_files,
    )


def _run_filter_repo(binary, source, transform, workdir, iteration):
    del binary
    repo = _working_repo(source, workdir / "repo")
    _warm_git_graph(repo, "refs/heads/source")
    command = [
        "git",
        "filter-repo",
        *transform.filter_repo,
        "--force",
        "--refs",
        "refs/heads/source",
    ]

    cold = _timed(command, repo)
    cold_files = _verify(source, source.base, transform, repo, "refs/heads/source")
    _remember_cold(repo, "refs/heads/source")
    run_args(["git", "reset", "--hard", source.tip], cwd=repo)
    _warm_git_graph(repo, f"{source.base}..{source.tip}")
    incremental = _timed(command, repo)
    incremental_files = _verify(
        source, source.tip, transform, repo, "refs/heads/source"
    )
    _verify_incremental(repo, "refs/heads/source")
    return _samples(
        source, transform, "git-filter-repo", iteration, cold, incremental,
        cold_files, incremental_files,
    )


def _run_filter_branch(binary, source, transform, workdir, iteration):
    del binary
    repo = _working_repo(source, workdir / "repo")
    _warm_git_graph(repo, "refs/heads/source")
    script = workdir / "index-filter.py"
    script.write_text(_INDEX_FILTER_SCRIPT)
    argument = {
        "subtree": source.spec.subtree,
        "prefix": PREFIX,
        "glob": source.spec.suffix,
    }[transform.name]
    index_filter = " ".join(
        shlex.quote(value)
        for value in (
            sys.executable,
            str(script.resolve()),
            transform.name,
            argument,
        )
    )
    command_prefix = [
        "git",
        "filter-branch",
        "--force",
        "--prune-empty",
        "--index-filter",
        index_filter,
        "--",
    ]
    cold_command = [*command_prefix, "refs/heads/source"]
    env = {"FILTER_BRANCH_SQUELCH_WARNING": "1"}

    cold = _timed(cold_command, repo, env=env)
    cold_files = _verify(source, source.base, transform, repo, "refs/heads/source")
    _remember_cold(repo, "refs/heads/source")
    _stage_filter_branch_increment(repo, source)
    incremental_command = [
        *command_prefix,
        "refs/heads/cold-output..refs/heads/source",
    ]
    incremental = _timed(incremental_command, repo, env=env)
    incremental_files = _verify(
        source, source.tip, transform, repo, "refs/heads/source"
    )
    _verify_incremental(repo, "refs/heads/source")
    return _samples(
        source, transform, "git-filter-branch", iteration, cold, incremental,
        cold_files, incremental_files,
    )


def _run_copybara(binary, source, transform, workdir, iteration):
    del binary
    if workdir.exists():
        shutil.rmtree(workdir)
    workdir.mkdir(parents=True)
    origin = _working_repo(source, workdir / "origin")
    _warm_git_graph(origin, "refs/heads/source")
    destination = workdir / "destination.git"
    run_args(["git", "init", "--bare", destination])
    config = workdir / "copy.bara.sky"
    config.write_text(_copybara_config(origin, destination, transform))
    base_command = ["copybara", "migrate", config, "--force", "--ignore-noop"]

    cold = _timed([*base_command, "--init-history"], workdir)
    cold_files = _verify(
        source, source.base, transform, destination, "refs/heads/main"
    )
    _remember_cold(destination, "refs/heads/main")
    run_args(["git", "reset", "--hard", source.tip], cwd=origin)
    _warm_git_graph(origin, f"{source.base}..{source.tip}")
    incremental = _timed(base_command, workdir)
    incremental_files = _verify(
        source, source.tip, transform, destination, "refs/heads/main"
    )
    _verify_incremental(destination, "refs/heads/main")
    return _samples(
        source, transform, "copybara", iteration, cold, incremental,
        cold_files, incremental_files,
    )


def _stage_filter_branch_increment(
    repo: Path,
    source: PreparedRepository,
) -> None:
    """Stage only the catch-up DAG on top of the cold filtered result."""
    originals = run_args(
        [
            "git",
            "rev-list",
            "--reverse",
            "--topo-order",
            f"{source.base}..{source.tip}",
        ],
        cwd=repo,
    ).splitlines()
    cold = run_args(
        ["git", "rev-parse", "refs/heads/cold-output"], cwd=repo
    ).strip()
    staged: dict[str, str] = {}
    commit_env = {
        "GIT_AUTHOR_NAME": "Josh benchmark",
        "GIT_AUTHOR_EMAIL": "benchmark@josh-project.github.io",
        "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z",
        "GIT_COMMITTER_NAME": "Josh benchmark",
        "GIT_COMMITTER_EMAIL": "benchmark@josh-project.github.io",
        "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z",
    }

    for original in originals:
        ancestry = run_args(
            ["git", "rev-list", "--parents", "-n", "1", original],
            cwd=repo,
        ).split()
        parents = []
        for parent in ancestry[1:]:
            mapped = staged.get(parent, cold)
            if mapped not in parents:
                parents.append(mapped)
        if not parents:
            parents.append(cold)
        tree = run_args(
            ["git", "rev-parse", f"{original}^{{tree}}"], cwd=repo
        ).strip()
        arguments = ["git", "commit-tree", tree]
        for parent in parents:
            arguments.extend(("-p", parent))
        staged[original] = run_args(
            arguments,
            cwd=repo,
            env=commit_env,
            stdin=f"Staged source commit {original}\n",
        ).strip()

    run_args(
        ["git", "update-ref", "refs/heads/source", staged[source.tip]],
        cwd=repo,
    )
    run_args(["git", "reset", "--hard", "refs/heads/source"], cwd=repo)


def _working_repo(source: PreparedRepository, path: Path) -> Path:
    repo = clone_pristine(source.path, path)
    run_args(["git", "checkout", "-B", "source", source.base], cwd=repo)
    return repo


def _warm_git_graph(repo: Path, revision: str) -> None:
    """Warm commit and tree objects without populating any tool cache."""
    subprocess.run(
        ["git", "rev-list", "--objects", revision],
        cwd=repo,
        check=True,
        stdout=subprocess.DEVNULL,
    )


def _timed(args, cwd, *, env=None) -> float:
    with Timer() as timer:
        run_args(args, cwd=cwd, env=env)
    return timer.elapsed


def _samples(
    source,
    transform,
    tool,
    iteration,
    cold,
    incremental,
    cold_files,
    incremental_files,
):
    common = (source.spec.name, transform.name, tool)
    return [
        RewriteSample(*common, "cold", iteration, cold, cold_files),
        RewriteSample(
            *common,
            "incremental",
            iteration,
            incremental,
            incremental_files,
        ),
    ]


def _tree_entries(repo: Path, revision: str) -> dict[str, tuple[str, str]]:
    raw = run_args(
        ["git", "ls-tree", "-r", "-z", "--full-tree", revision], cwd=repo
    )
    entries = {}
    for record in raw.split("\0"):
        if not record:
            continue
        metadata, path = record.split("\t", 1)
        mode, _kind, oid = metadata.split()
        entries[path] = (mode, oid)
    return entries


def _expected_entries(source, revision, transform):
    entries = _tree_entries(source.path, revision)
    if transform.name == "subtree":
        prefix = f"{source.spec.subtree}/"
        return {
            path.removeprefix(prefix): value
            for path, value in entries.items()
            if path.startswith(prefix)
        }
    if transform.name == "prefix":
        return {f"{PREFIX}/{path}": value for path, value in entries.items()}

    suffix = source.spec.suffix
    return {
        path: value
        for path, value in entries.items()
        if path.endswith(suffix)
        and not any(part.startswith(".") for part in path.split("/"))
    }


def _verify(source, revision, transform, repo, result_ref):
    expected = _expected_entries(source, revision, transform)
    actual = _tree_entries(repo, result_ref)
    if actual != expected:
        missing = sorted(expected.keys() - actual.keys())[:5]
        extra = sorted(actual.keys() - expected.keys())[:5]
        changed = sorted(
            path
            for path in expected.keys() & actual.keys()
            if expected[path] != actual[path]
        )[:5]
        raise AssertionError(
            f"{source.spec.name}/{transform.name}: output tree mismatch; "
            f"missing={missing}, extra={extra}, changed={changed}"
        )
    return len(actual)



def _remember_cold(repo: Path, result_ref: str) -> None:
    run_args(
        ["git", "update-ref", "refs/heads/cold-output", result_ref],
        cwd=repo,
    )


def _verify_incremental(repo: Path, result_ref: str) -> None:
    run_args(
        [
            "git",
            "merge-base",
            "--is-ancestor",
            "refs/heads/cold-output",
            result_ref,
        ],
        cwd=repo,
    )

_INDEX_FILTER_SCRIPT = r"""\
import os
import subprocess
import sys

kind, argument = sys.argv[1:]
index = os.environ["GIT_INDEX_FILE"]
temporary = f"{index}.benchmark"
try:
    os.unlink(temporary)
except FileNotFoundError:
    pass

raw = subprocess.check_output(["git", "ls-files", "-s", "-z"])
records = []
argument_bytes = argument.encode()
for record in raw.split(b"\0"):
    if not record:
        continue
    metadata, path = record.split(b"\t", 1)
    if kind == "subtree":
        prefix = argument_bytes + b"/"
        if not path.startswith(prefix):
            continue
        path = path.removeprefix(prefix)
    elif kind == "prefix":
        path = argument_bytes + b"/" + path
    elif (
        not path.endswith(argument_bytes)
        or any(part.startswith(b".") for part in path.split(b"/"))
    ):
        continue
    records.append(metadata + b"\t" + path)

environment = os.environ.copy()
environment["GIT_INDEX_FILE"] = temporary
subprocess.run(["git", "read-tree", "--empty"], check=True, env=environment)
if records:
    subprocess.run(
        ["git", "update-index", "-z", "--index-info"],
        check=True,
        env=environment,
        input=b"\0".join(records) + b"\0",
    )
os.replace(temporary, index)
"""


def _copybara_config(origin, destination, transform):
    transformations = ",\n".join(
        f"        {item}" for item in transform.copybara_transformations
    )
    if transformations:
        transformations += ",\n"
    return f"""\
core.workflow(
    name = "default",
    origin = git.origin(
        url = "file://{origin.resolve()}",
        ref = "refs/heads/source",
    ),
    origin_files = {transform.copybara_files},
    destination = git.destination(
        url = "file://{destination.resolve()}",
        fetch = "refs/heads/main",
        push = "refs/heads/main",
    ),
    authoring = authoring.pass_thru(default = "Copybara <copybara@example.com>"),
    mode = "ITERATIVE",
    transformations = [
{transformations}    ],
)
"""
