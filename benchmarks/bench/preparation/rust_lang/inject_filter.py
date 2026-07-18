"""Inject a per-era stored filter (``bench.josh``) into a rust-lang/rust clone.

Stage 1 of the rust-lang benchmark: overlay a root-level ``bench.josh`` stored
filter whose content depends on the era a commit falls in, as if the repo had
been developed with josh from the start. The result is written to a
caller-chosen ref (``refs/heads/injected``); the input history is untouched.
Stage 2 consumes it via the stored-filter operator ``:+bench``.

A single ``:rev`` filter selects the right per-era blob per commit; see
:func:`build_rev_filter` for its layout. Two josh operators do the work:

- ``:$<path>=<OID>`` inserts a blob by SHA at ``<path>``
  (grammar ``josh-filter/src/flang/grammar.pest``; test ``tests/filter/insert.t``).
- ``:rev(<tip>:<filter>, ...)`` picks a sub-filter per commit, first-match-wins,
  ``<`` = strict-ancestor, ``_`` = default
  (``josh-core/src/filter/mod.rs``; test ``tests/filter/rev.t``).
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path

from bench.shell import run

# Persisted partitions + per-era filter files, at bench/data/rust_lang.
DATA_DIR = Path(__file__).resolve().parent.parent.parent / "data" / "rust_lang"

# Default ref the injected history is written to.
DEFAULT_REF = "refs/heads/injected"

# In-tree path of the injected stored filter. It sits at the repo root (no
# ``.josh`` extension needed by the caller), so stage 2 consumes it via
# ``:+bench``. Stored filters apply from the root, matching the ``:/...`` paths
# in the per-era filter bodies.
STORED_FILTER_PATH = "bench.josh"


@dataclass
class Era:
    """One era of the filter partition: starts at ``commit`` (inclusive)."""

    id: str
    commit: str
    filter_file: str  # basename inside DATA_DIR

    @property
    def filter_path(self) -> Path:
        return DATA_DIR / self.filter_file


@dataclass
class InjectionResult:
    """Outcome of :func:`inject_filter`."""

    ref: str
    """Ref holding the injected history."""

    head_oid: str
    """Original (un-injected) HEAD SHA the injection ran against."""

    injected_head_oid: str
    """SHA of the rewritten HEAD under ``ref``."""

    blob_oids: dict[str, str]
    """Per-era filter blob OIDs as hashed into the repo (era id -> OID)."""

    filter_path: Path
    """Temp file holding the generated ``:rev`` filter spec."""


def load_eras(data_dir: Path = DATA_DIR) -> list[Era]:
    """Read ``partitions.toml`` and return the eras, oldest first."""
    with (data_dir / "partitions.toml").open("rb") as f:
        data = tomllib.load(f)
    return [
        Era(
            id=str(e["id"]),
            commit=str(e["commit"]),
            filter_file=str(e["filter"]),
        )
        for e in data["era"]
    ]


def hash_filter_blobs(repo: str | Path, eras: list[Era]) -> dict[str, str]:
    """``git hash-object -w`` each era's filter file INTO ``repo``.

    Returns era id -> blob OID. Idempotent: re-hashing an identical file
    yields the same OID, and the object store dedupes.
    """
    oids: dict[str, str] = {}
    for era in eras:
        # ``-w`` writes the blob into the repo's object store; ``--`` guards
        # against filenames that look like flags.
        oid = run(
            f"git hash-object -w -- {era.filter_path!s}",
            cwd=str(repo),
        ).strip()
        oids[era.id] = oid
    return oids


def build_rev_filter(eras: list[Era], blob_oids: dict[str, str]) -> str:
    """Construct the ``:rev(...)`` filter string (identity prepend + eras).

    Layout (first-match-wins, strict-ancestor ``<``), with ``SF = bench.josh``:

    - ``<P_0:/``                       identity for pre-P_0 history
    - ``<P_1:[:/,:$SF=OID_0]``         era 0 (P_0 inclusive .. P_1 exclusive)
    - ``<P_2:[:/,:$SF=OID_1]``         era 1
    - ...
    - ``_:[:/,:$SF=OID_N>``            last era, from P_N through HEAD

    Note the one-step shift: the entry tipped at ``P_{i+1}`` carries era ``i``'s
    content, because era ``i`` spans ``[P_i, P_{i+1})`` and a commit in that
    range is a strict ancestor of ``P_{i+1}`` but not of any earlier tip. The
    first entry (``<P_0:/``) is identity, so ``bench.josh`` does NOT exist
    before ``P_0``.
    """
    if not eras:
        raise ValueError("need at least one era")

    sf_path = STORED_FILTER_PATH
    entries: list[str] = []
    # Identity entry: strict ancestors of P_0 get no bench.josh.
    entries.append(f"  <{eras[0].commit}:/")
    # era i's content applies over [P_i, P_{i+1}), the range selected by the
    # entry tipped at P_{i+1} (the one-step shift, see docstring).
    for i in range(len(eras) - 1):
        tip = eras[i + 1].commit
        oid = blob_oids[eras[i].id]
        entries.append(rf"  <{tip}:[:/,:${sf_path}={oid}]")
    # Default entry: the last era applies from its own commit through HEAD.
    last_oid = blob_oids[eras[-1].id]
    entries.append(rf"  _:[:/,:${sf_path}={last_oid}]")

    return ":rev(\n" + "\n".join(entries) + "\n)"


def inject_filter(
    binary: str | Path,
    repo: str | Path,
    *,
    ref: str = DEFAULT_REF,
    input_ref: str = "HEAD",
    data_dir: Path = DATA_DIR,
    keep_filter_file: Path | None = None,
) -> InjectionResult:
    """Inject the per-era ``bench.josh`` into ``repo``, writing the result to ``ref``.

    Parameters mirror the rest of the harness: ``binary`` is the josh-filter
    release binary, ``repo`` is a rust-lang/rust clone (used as cwd, since
    josh-filter resolves the repo via ``git2::open_from_env``). The per-era
    filter blobs are hashed into ``repo`` and a single ``:rev`` filter is applied.

    The full history (~330k commits for rust-lang) may take a few minutes.

    Returns the injected ref, the original HEAD SHA, the rewritten HEAD SHA,
    the per-era blob OIDs, and the path the filter spec was written to.
    """
    repo = Path(repo)
    binary = Path(binary)

    eras = load_eras(data_dir)
    blob_oids = hash_filter_blobs(repo, eras)
    filter_spec = build_rev_filter(eras, blob_oids)

    # Write the filter to a file so the spec survives shell-quoting (note that
    # ``-s`` is ``--cache-stats`` in josh-filter, NOT ``--spec``; the filter
    # comes from ``--file``).
    filter_path = keep_filter_file if keep_filter_file is not None else Path(
        "/tmp", "josh_inject_filter.josh"
    )
    filter_path.parent.mkdir(parents=True, exist_ok=True)
    filter_path.write_text(filter_spec)

    # Pretty-print parses the filter before the heavy run, failing early with a
    # clear error if the spec is malformed.
    run(f"{binary} -p --file {filter_path}", cwd=str(repo))

    head_oid = run("git rev-parse HEAD", cwd=str(repo)).strip()
    out = run(
        f"{binary} --file {filter_path} {input_ref} --update {ref}",
        cwd=str(repo),
    ).strip()
    # josh-filter prints the rewritten HEAD SHA on the last non-empty line.
    injected_head_oid = out.splitlines()[-1].strip()

    return InjectionResult(
        ref=ref,
        head_oid=head_oid,
        injected_head_oid=injected_head_oid,
        blob_oids=blob_oids,
        filter_path=filter_path,
    )
