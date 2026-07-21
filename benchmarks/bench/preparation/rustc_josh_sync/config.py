"""Subtree definitions and josh filter construction for the rustc-josh-sync scenario.

rust-lang syncs subtrees of rust-lang/rust with the `rustc-josh-sync` tool
(https://github.com/rust-lang/josh-sync). Each synced subtree carries its sync
configuration at ``<path>/josh-sync.toml`` inside rust-lang/rust, so the exact
filter each repo uses can be read from the pinned tree instead of hardcoding it.

The filter construction below is a direct port of josh-sync's ``config.rs``
(``construct_josh_filter`` / ``convert_rev_syntax`` / ``wrap_compat``) so the
benchmark filters match production byte for byte.
"""

import os
import re
import tomllib
from pathlib import Path

from bench.shell import run

RUST_REMOTE = "https://github.com/rust-lang/rust"

# rust-lang/rust main tip as of 2026-07-19. Newer than the rust-lang scenario's
# pin on purpose: stdarch/compiler-builtins only adopted josh-sync in 2025, and
# the replay mining needs >= 10 syncs per subtree in both directions.
RUST_REVISION = "234c31cd674e11703f15d290cba7ff81dfe8b4b8"

# josh built at this commit is both the subject under test (josh-proxy, josh
# CLI) and the producer of the reference filtered histories (josh-filter);
# keeping it one commit makes SHA-exact verification possible. Pinned to
# master as of 2026-07-19, which includes the fix for the CLI dropping
# semantic meta args ("Apply semantic meta args of remote filters in the josh
# CLI") -- older commits fail the CLI pass's SHA-exact verification.
# Override with SYNC_JOSH_COMMIT (a full commit SHA) to benchmark another josh
# version; filtered-history and manifest caches are keyed by it, so runs with
# different versions do not mix.
JOSH_COMMIT = os.environ.get(
    "SYNC_JOSH_COMMIT", "d7649b7e1ad59537b7b2ed131e5bfb974fe0c5b4"
)

# Served under this path so proxy URLs match production shape
# (http://<josh>/rust-lang/rust.git<filter>.git).
UPSTREAM_REPO = "rust-lang/rust"

# Benchmarked subtrees: name -> path inside rust-lang/rust.
SUBTREES = {
    "rustc-dev-guide": "src/doc/rustc-dev-guide",
    "stdarch": "library/stdarch",
    "compiler-builtins": "library/compiler-builtins",
    "miri": "src/tools/miri",
}


def load_filter(rust_repo: str | Path, revision: str, path: str) -> str:
    """Build the exact josh filter rustc-josh-sync would use for `path`.

    Reads ``<path>/josh-sync.toml`` from `revision` in `rust_repo` and applies
    the same construction as josh-sync's ``JoshConfig::construct_josh_filter``.
    """
    raw = run(f"git show {revision}:{path}/josh-sync.toml", cwd=str(rust_repo))
    config = tomllib.loads(raw)

    cfg_path, cfg_filter = config.get("path"), config.get("filter")
    if cfg_filter is not None and cfg_path is None:
        spec = cfg_filter
    elif cfg_path is not None and cfg_filter is None:
        spec = f":/{cfg_path}"
    else:
        raise ValueError(f"{path}/josh-sync.toml must set exactly one of path/filter")

    return _wrap_compat(_convert_rev_syntax(spec))


# ``:rev(<sha>:<filter>)`` in josh-sync configs uses a legacy syntax; josh
# itself expects ``:rev(<=<sha>:<filter>)``, and the all-zero SHA means "the
# root" and is spelled ``_``. Port of josh-sync's ``convert_rev_syntax``.
_REV_BLOCK = re.compile(r":rev\([^)]*\)")
_REV_ENTRY = re.compile(r"([,(])(0{40}|[0-9a-f]{40}):")


def _convert_rev_syntax(spec: str) -> str:
    def convert_entry(m: re.Match[str]) -> str:
        delim, sha = m.group(1), m.group(2)
        if set(sha) == {"0"}:
            return f"{delim}_:"
        return f"{delim}<={sha}:"

    return _REV_BLOCK.sub(lambda m: _REV_ENTRY.sub(convert_entry, m.group(0)), spec)


def _wrap_compat(spec: str) -> str:
    return f':~(history="keep-trivial-merges",gpgsig="norm-lf")[{spec}]'
