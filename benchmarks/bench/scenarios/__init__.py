"""Benchmark scenarios, keyed by CLI name."""

from collections.abc import Callable

from bench.scenarios import rust_lang, select_folders

SCENARIOS: dict[str, Callable[[], object]] = {
    "select-folders": select_folders.run,
    "rust-lang": rust_lang.run,
}
