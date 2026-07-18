"""rust-lang: filter a stable {core, alloc, std, rustc_ast} view via a stored filter.

josh-only stage of the rust-lang/rust benchmark. Preparation (untimed) injects a
per-era ``bench.josh`` stored filter into a pristine bare copy of rust-lang's
history; this scenario then times josh-filter materializing the stable view from
that injected history. The comparison tools land in a later stage.
"""

from bench.build import build_josh_filter
from bench.git import verify_result
from bench.paths import TARGET_DIR
from bench.preparation.rust_lang.rust_prepare import prepare_injected_repo
from bench.result import ToolResult
from bench.tools.josh_filter import run_josh_filter

# One binary does both the injection (preparation) and the timed filter.
JOSH_COMMIT = "43ed8cb6e905730f4ccd261ba8bc03cddea6c78d"

# :+bench applies the stored filter bench.josh, read per commit from the tree, so
# each commit picks up its own era's filter and the materialized layout stays stable.
FILTER_SPEC = ":+bench"

# The injected history lives on this ref, NOT HEAD (HEAD still points at the
# original upstream tip, which has no bench.josh).
INJECTED_REF = "refs/heads/injected"

# Lexical (git tree) order, matching `git ls-tree --name-only`. The stored filter
# file itself is carried through in the output, at the root as bench.josh.
EXPECTED_TOP_LEVEL = "alloc\nbench.josh\ncore\nrustc_ast\nstd"


def run() -> list[ToolResult]:
    """Build josh, prepare the injected repo (untimed), then time the filter."""
    josh_bin = build_josh_filter(JOSH_COMMIT, TARGET_DIR)
    # Untimed: clean bare source -> pristine copy -> inject -> repack.
    injected = prepare_injected_repo(josh_bin, TARGET_DIR)

    work = TARGET_DIR / "work" / "rust_lang"
    result = run_josh_filter(
        josh_bin,
        injected,
        FILTER_SPEC,
        work / "josh-filter",
        ref="refs/heads/view",
        input_ref=INJECTED_REF,
        bare=True,
    )
    verify_result(result, expected_top_level=EXPECTED_TOP_LEVEL)
    return [result]
