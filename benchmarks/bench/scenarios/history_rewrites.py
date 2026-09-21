"""Compare native history rewriting across repositories and tools.

Each sample pair starts from fresh tool state. The cold phase rewrites history
through a base 100 first-parent commits behind the source tip. The incremental
phase advances to that tip using the same tool state. git-filter-branch stages
the catch-up commit graph onto its cold filtered result before timing.
"""

import csv
import json
import os
from dataclasses import asdict
from pathlib import Path

import pandas as pd

from bench.build import build_latest_josh_filter
from bench.chart import grouped_chart, save_chart
from bench.duration import format_duration
from bench.paths import OUTPUT_DIR, TARGET_DIR
from bench.preparation.history_rewrites import REPOSITORIES, prepare_repository
from bench.tools.history_rewrite import (
    ALL_TOOLS,
    FAST_TOOLS,
    RewriteSample,
    median_rows,
    preflight_tool,
    run_pair,
    transformations,
)

TRANSFORMATIONS = ("subtree", "prefix", "glob")


def _selected(env_name: str, known) -> list[str]:
    configured = os.environ.get(env_name)
    if not configured:
        return list(known)
    selected = [item.strip() for item in configured.split(",") if item.strip()]
    unknown = [item for item in selected if item not in known]
    if unknown:
        choices = ", ".join(known)
        raise SystemExit(f"unknown {env_name} entries {unknown}; known: {choices}")
    return selected


def run() -> list[RewriteSample]:
    """Prepare fixtures, execute selected sample pairs, verify, and report."""
    repository_names = _selected("HISTORY_REWRITE_REPOS", REPOSITORIES)
    tool_names = _selected("HISTORY_REWRITE_TOOLS", ALL_TOOLS)
    transformation_names = _selected(
        "HISTORY_REWRITE_TRANSFORMS", TRANSFORMATIONS
    )
    iterations = int(os.environ.get("HISTORY_REWRITE_RUNS", "1"))
    if iterations < 1:
        raise SystemExit("HISTORY_REWRITE_RUNS must be at least 1")
    increment_commits = int(
        os.environ.get("HISTORY_REWRITE_INCREMENT_COMMITS", "100")
    )
    if increment_commits < 1:
        raise SystemExit("HISTORY_REWRITE_INCREMENT_COMMITS must be at least 1")
    smoke = os.environ.get("HISTORY_REWRITE_SMOKE") == "1"

    josh_binary = Path()
    josh_tag = None
    josh_commit = None
    if "josh" in tool_names:
        josh_binary, josh_tag, josh_commit = build_latest_josh_filter(TARGET_DIR)
        print(f"Josh release: {josh_tag} ({josh_commit})", flush=True)

    prepared = {
        name: prepare_repository(
            REPOSITORIES[name],
            TARGET_DIR,
            increment_commits=increment_commits,
            smoke=smoke,
        )
        for name in repository_names
    }
    for name, source in prepared.items():
        print(
            f"{name}: {source.range_commits} commits in incremental range",
            flush=True,
        )
    samples: list[RewriteSample] = []
    work_root = TARGET_DIR / "work" / "history_rewrites"
    active_tools = [
        tool
        for tool in tool_names
        if any(
            not REPOSITORIES[name].fast_tools_only or tool in FAST_TOOLS
            for name in repository_names
        )
    ]
    for tool in active_tools:
        preflight_tool(tool, josh_binary, work_root / "preflight" / tool)

    for repository_name in repository_names:
        source = prepared[repository_name]
        available_tools = FAST_TOOLS if source.spec.fast_tools_only else ALL_TOOLS
        selected_tools = [tool for tool in tool_names if tool in available_tools]
        skipped = [tool for tool in tool_names if tool not in available_tools]
        if skipped:
            print(
                f"{repository_name}: skipping slow tools {', '.join(skipped)}",
                flush=True,
            )

        native_transforms = transformations(source)
        for transformation_name in transformation_names:
            transform = native_transforms[transformation_name]
            for tool in selected_tools:
                for iteration in range(1, iterations + 1):
                    workdir = (
                        work_root
                        / repository_name
                        / transformation_name
                        / tool
                        / str(iteration)
                    )
                    pair = run_pair(
                        tool,
                        josh_binary,
                        source,
                        transform,
                        workdir,
                        iteration,
                    )
                    samples.extend(pair)
                    for sample in pair:
                        print(
                            f"{sample.repository}/{sample.transformation}/"
                            f"{sample.tool} {sample.phase}: "
                            f"{format_duration(sample.elapsed)} "
                            f"({sample.files} files)",
                            flush=True,
                        )

    if not samples:
        raise SystemExit("the repository and tool selections produced no benchmark runs")

    _report(samples, prepared, josh_tag, josh_commit, smoke)
    return samples


def _report(samples, prepared, josh_tag, josh_commit, smoke):
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    raw_samples = [asdict(sample) for sample in samples]
    metadata = {
        "josh_release": (
            {"tag": josh_tag, "commit": josh_commit} if josh_tag else None
        ),
        "smoke": smoke,
        "repositories": {
            name: {
                "remote": source.spec.remote,
                "upstream_revision": source.upstream_revision,
                "base": source.base,
                "incremental_tip": source.tip,
                "range_commits": source.range_commits,
            }
            for name, source in prepared.items()
        },
    }

    json_path = OUTPUT_DIR / "history_rewrites_samples.json"
    json_path.write_text(
        json.dumps({"metadata": metadata, "samples": raw_samples}, indent=2) + "\n"
    )
    csv_path = OUTPUT_DIR / "history_rewrites_samples.csv"
    with csv_path.open("w", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=list(raw_samples[0]))
        writer.writeheader()
        writer.writerows(raw_samples)

    rows = median_rows(samples)
    print("\nMedian elapsed time:")
    for row in rows:
        print(
            f"  {row['repository']}/{row['transformation']}/"
            f"{row['tool']} {row['phase']}: "
            f"{format_duration(row['elapsed_s'])} (n={row['samples']})"
        )

    chart_rows = [
        {
            "group": (
                f"{row['repository']}: {row['transformation']} {row['phase']}"
            ),
            "series": row["tool"],
            "elapsed_s": row["elapsed_s"],
        }
        for row in rows
    ]
    series_order = list(dict.fromkeys(sample.tool for sample in samples))
    chart = grouped_chart(
        pd.DataFrame(chart_rows),
        series_order,
        "Native history rewrite benchmarks (median, lower is better)",
    )
    chart_path = save_chart(chart, OUTPUT_DIR / "history_rewrites.png")
    print(f"samples written to {json_path} and {csv_path}")
    print(f"chart written to {chart_path}")
