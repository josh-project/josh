"""Prepare the rust-lang/rust source and the injected stored-filter repo.

This is the PREPARATION phase of the rust-lang benchmark: produce a cached,
repacked clone of rust-lang/rust and inject the per-era ``bench.josh`` stored
filter into it, yielding a repo that looks as if it had been developed with josh
from the start. No timing or tool comparison happens here -- the timed benchmark
(stage 2) consumes ``refs/heads/injected`` from the repo returned by
:func:`prepare_injected_repo`.

See :mod:`bench.preparation.rust_lang.inject_filter` for the injection mechanism and
``bench/data/rust_lang/`` for the per-era filter contents.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

from bench.git import clone_pristine, fetch_repo
from bench.preparation.rust_lang.inject_filter import DEFAULT_REF, inject_filter
from bench.shell import run

# rust-lang/rust pinned for a reproducible prepared artifact; cached locally
# after the first fetch so the large download only happens once.
RUST_REMOTE = "https://github.com/rust-lang/rust"
RUST_REVISION = "656ccbe796ff98def9b555c118e1620c5389e3b2"


def _reachable(repo: Path, revision: str) -> bool:
    """True if `repo` exists as a git repo and contains `revision`."""
    if not (repo / "objects").exists():
        return False
    # `cat-file -e` exits 0 iff the object is present.
    return (
        subprocess.run(
            ["git", "-C", str(repo), "cat-file", "-e", revision],
            capture_output=True,
        ).returncode
        == 0
    )


def _ref_exists(repo: Path, ref: str) -> bool:
    """True if `ref` resolves in `repo`."""
    return (
        subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "--verify", "-q", ref],
            capture_output=True,
        ).returncode
        == 0
    )


def prepare_rust_source(
    target_dir: str | Path,
    *,
    revision: str = RUST_REVISION,
    remote: str = RUST_REMOTE,
    bare: bool = True,
    force: bool = False,
) -> Path:
    """Clone rust-lang at `revision` into a cached, repacked repo; return its path.

    The cache lives at ``<target_dir>/rust-lang-source/<short-sha>``. On a cache
    hit (the dir exists and contains `revision`, and `force` is False) the existing
    repo is reused unchanged. On a miss it is fetched (bare by default) and
    repacked into a single pack.

    Pass ``remote=<path>`` to clone from a local checkout (e.g. a
    ``~/Projects/rust-lang`` clone) instead of github, avoiding the network fetch.

    The repo is bare by default: the only preparation consumer is josh (the
    injection), which works on git objects with no working tree.
    """
    target_dir = Path(target_dir)
    cache_dir = target_dir / "rust-lang-source" / revision[:12]

    if not force and _reachable(cache_dir, revision):
        return cache_dir

    # fetch_repo recreates cache_dir from scratch, so a partial/interrupted
    # cache is replaced cleanly. It creates `<target_dir>/<name>`; pass the
    # cache dir as (parent, leaf name).
    fetch_repo(remote, cache_dir.name, revision, cache_dir.parent, bare=bare)

    # Consolidate into one tight pack for faster subsequent reads.
    run("git repack -a -d -q", cwd=str(cache_dir))

    return cache_dir


def prepare_injected_repo(
    josh_binary: str | Path,
    target_dir: str | Path,
    *,
    revision: str = RUST_REVISION,
    remote: str = RUST_REMOTE,
    bare: bool = True,
    force: bool = False,
) -> Path:
    """Prepare a pristine copy of the rust-lang source with ``bench.josh`` injected.

    Injection is done on a SEPARATE bare copy at
    ``<target_dir>/rust-lang-injected/<short-sha>`` so the source cache stays
    pristine (never carrying ``refs/heads/injected``). The copy is repacked after
    injection. Returns the injected repo path.

    Idempotent: if the injected repo already carries ``refs/heads/injected`` and
    ``force`` is False, it is returned unchanged.

    ``force`` re-copies the source and re-runs the injection (e.g. after editing
    the persisted ``filter_*.josh`` contents); it does NOT re-fetch the source.
    To rebuild the source cache, call :func:`prepare_rust_source` with
    ``force=True`` first.
    """
    target_dir = Path(target_dir)
    injected_dir = target_dir / "rust-lang-injected" / revision[:12]

    if not force and _ref_exists(injected_dir, DEFAULT_REF):
        return injected_dir

    source = prepare_rust_source(
        target_dir, revision=revision, remote=remote, bare=bare
    )

    # Inject into a copy so the source cache is never polluted with the
    # injected ref.
    clone_pristine(source, injected_dir, bare=bare)
    inject_filter(josh_binary, injected_dir)

    # Injection leaves objects in loose/extra packs; consolidate into one tight
    # pack so stage-2 clones and filtering read fast.
    run("git repack -a -d -q", cwd=str(injected_dir))

    return injected_dir
