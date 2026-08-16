"""Benchmark scenarios, keyed by CLI name."""

from collections.abc import Callable

from bench.scenarios import rust_lang, rustc_josh_sync, select_folders

SCENARIOS: dict[str, Callable[[], object]] = {
    "select-folders": select_folders.run,
    "rust-lang": rust_lang.run,
    "rustc-josh-sync": rustc_josh_sync.run,
}
