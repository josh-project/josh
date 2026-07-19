"""rustc-josh-sync: replay real rust-lang subtree syncs through josh.

End-to-end benchmark of the workflow rust-lang uses to sync subtrees
(rustc-dev-guide, stdarch, compiler-builtins, miri) with rust-lang/rust via the
`rustc-josh-sync` tool. Real historical syncs are mined from the sync commits in
rust-lang/rust history and replayed in both directions against a locally served
mirror, once per tool:

- ``proxy``: the production setup -- git talks to a josh-proxy which filters
  server-side (pull) and reverse-filters pushes onto rust history. Replays
  against the production-filter reference (compat wrapper included).
- ``cli``: the josh CLI in the subtree checkout talking directly to the
  unfiltered upstream -- ``josh fetch`` transfers the raw history and filters
  locally, ``josh push`` reverse-filters locally.
- ``cli-nodc``: same as ``cli`` but with ``--no-distributed-cache``, which
  drops the DistributedCacheBackend from the cache stack and skips fetching
  the remote josh cache before filtering.

Per subtree and tool the replay starts cold (fresh proxy cache / fresh work
repo, so the first event -- always a pull -- pays the full-history cost) and
stays warm for the remaining events, matching the steady state of
josh.rust-lang.org for the proxy and of a long-lived checkout for the CLI.

Environment knobs:
- ``SYNC_REPLAY_DEPTH``: historical syncs per subtree per direction (default 10)
- ``SYNC_SUBTREES``: comma-separated subset of subtrees for quick runs
- ``SYNC_TOOLS``: comma-separated subset of {proxy, cli, cli-nodc}
  (default all)
- ``SYNC_JOSH_COMMIT``: josh version to benchmark (see config.py)
"""

import functools
import json
import os
import statistics

import pandas as pd

from bench.build import build_josh_proxy
from bench.chart import grouped_chart, save_chart
from bench.duration import format_duration
from bench.paths import OUTPUT_DIR, TARGET_DIR
from bench.preparation.rust_lang.rust_prepare import prepare_rust_source
from bench.preparation.rustc_josh_sync.config import (
    JOSH_COMMIT,
    RUST_REMOTE,
    RUST_REVISION,
    SUBTREES,
    UPSTREAM_REPO,
    load_filter,
)
from bench.preparation.rustc_josh_sync.mine import mine_subtree
from bench.preparation.rustc_josh_sync.prepare import (
    prepare_filtered,
    prepare_upstream,
    prepare_work_clone,
    remove_dir,
)
from bench.proxy import GitHttpServer, JoshProxy
from bench.tools.josh_cli_sync import replay_pull_cli, replay_push_cli, setup_cli_remote
from bench.tools.josh_proxy_sync import SyncSample, replay_pull, replay_push

TOOLS = ("proxy", "cli", "cli-nodc")

# Chart series, ordered so adjacent bars compare the tools.
SERIES = [
    "proxy pull (cold)", "cli pull (cold)", "cli-nodc pull (cold)",
    "proxy pull (warm)", "cli pull (warm)", "cli-nodc pull (warm)",
    "proxy push (warm)", "cli push (warm)", "cli-nodc push (warm)",
]


def _selected(env: str, known: tuple[str, ...] | dict) -> list[str]:
    picked = os.environ.get(env)
    if not picked:
        return list(known)
    names = [n.strip() for n in picked.split(",") if n.strip()]
    unknown = [n for n in names if n not in known]
    if unknown:
        raise SystemExit(f"unknown {env} entries {unknown}; known: {', '.join(known)}")
    return names


def _replay_proxy(binaries, serve_root, upstream, work, manifest, name, work_dir):
    samples = []
    cache_dir = work_dir / "cache" / name
    remove_dir(cache_dir)  # fresh proxy cache: first event is cold
    with JoshProxy(binaries, serve_root, cache_dir, work_dir / "logs" / name) as proxy:
        for i, ev in enumerate(manifest["events"]):
            if ev["kind"] == "pull":
                elapsed, verified = replay_pull(proxy, work, manifest["filter"], ev)
            else:
                elapsed, verified = replay_push(
                    proxy, work, upstream, manifest["filter"], ev,
                    branch=f"bench-proxy-{name}-{i}",
                )
            samples.append(("proxy", i, ev, elapsed, verified))
    return samples


def _replay_cli(
    binaries, serve_root, upstream, work, manifest, name, work_dir,
    tool="cli", josh_args="",
):
    samples = []
    # The runner interpolates `josh` into shell commands, so global flags can
    # ride along with the binary path.
    josh = f"{binaries['josh']} {josh_args}".strip()
    with GitHttpServer(
        binaries, serve_root, work_dir / "logs" / f"{name}-{tool}"
    ) as gitd:
        setup_cli_remote(josh, work, gitd.url(UPSTREAM_REPO), manifest["filter"])
        for i, ev in enumerate(manifest["events"]):
            if ev["kind"] == "pull":
                # One advancing pull branch per subtree, like a real upstream.
                elapsed, verified = replay_pull_cli(
                    josh, work, upstream, ev, branch=f"sync-{tool}-{name}"
                )
            else:
                elapsed, verified = replay_push_cli(
                    josh, work, upstream, ev, branch=f"bench-{tool}-{name}-{i}"
                )
            samples.append((tool, i, ev, elapsed, verified))
    return samples


def run() -> list[SyncSample]:
    depth = int(os.environ.get("SYNC_REPLAY_DEPTH", "10"))
    names = _selected("SYNC_SUBTREES", SUBTREES)
    tools = _selected("SYNC_TOOLS", TOOLS)

    binaries = build_josh_proxy(JOSH_COMMIT, TARGET_DIR)
    source = prepare_rust_source(TARGET_DIR, revision=RUST_REVISION, remote=RUST_REMOTE)

    manifests = {}
    for name in names:
        spec = load_filter(source, RUST_REVISION, SUBTREES[name])
        filtered = prepare_filtered(binaries["josh-filter"], source, name, spec, TARGET_DIR)
        manifests[name] = (
            mine_subtree(
                name, source, filtered, spec, SUBTREES[name],
                binaries["josh-filter"], JOSH_COMMIT, depth, TARGET_DIR,
            ),
            filtered,
        )

    work_dir = TARGET_DIR / "work" / "rustc_josh_sync"
    serve_root, upstream = prepare_upstream(source, work_dir)
    replayers = {
        "proxy": _replay_proxy,
        "cli": _replay_cli,
        "cli-nodc": functools.partial(
            _replay_cli, tool="cli-nodc", josh_args="--no-distributed-cache"
        ),
    }

    samples: list[SyncSample] = []
    for name in names:
        for tool in tools:
            manifest, filtered = manifests[name]
            # A fresh work clone per pass: the CLI's caches live in the repo,
            # and both tools must start equally cold.
            work = prepare_work_clone(filtered, name, work_dir)
            raw = replayers[tool](
                binaries, serve_root, upstream, work, manifest, name, work_dir
            )
            for tool_name, i, ev, elapsed, verified in raw:
                cache = "cold" if i == 0 else "warm"
                samples.append(
                    SyncSample(tool_name, name, ev["kind"], cache, elapsed, ev, verified)
                )
                note = "" if verified == "ok" else f"  [warn: {verified}]"
                print(
                    f"{name}/{tool_name}: {ev['kind']} {cache} "
                    f"{format_duration(elapsed)}{note}",
                    flush=True,
                )

    _report(samples, names, tools)
    return samples


def _stats(values: list[float]) -> str:
    if not values:
        return "-"
    med = statistics.median(values)
    return f"{format_duration(med)} (min {format_duration(min(values))}, " \
           f"max {format_duration(max(values))}, n={len(values)})"


def _report(samples: list[SyncSample], names: list[str], tools: list[str]) -> None:
    raw = [
        {
            "tool": s.tool,
            "subtree": s.subtree,
            "direction": s.direction,
            "cache": s.cache,
            "elapsed_s": s.elapsed,
            "verified": s.verified,
            "event": s.event.get("hist_merge") or s.event.get("hist_rust_merge"),
        }
        for s in samples
    ]
    out = OUTPUT_DIR / "rustc_josh_sync_samples.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(raw, indent=2))
    print(f"\nsamples written to {out}")

    rows = []
    for name in names:
        print(f"{name}:")
        for tool in tools:
            picked = [s for s in samples if s.subtree == name and s.tool == tool]
            cold = [s.elapsed for s in picked if s.cache == "cold"]
            warm_pull = [
                s.elapsed for s in picked if s.cache == "warm" and s.direction == "pull"
            ]
            warm_push = [
                s.elapsed for s in picked if s.cache == "warm" and s.direction == "push"
            ]
            print(f"  {tool} pull cold: {format_duration(cold[0]) if cold else '-'}")
            print(f"  {tool} pull warm: {_stats(warm_pull)}")
            print(f"  {tool} push warm: {_stats(warm_push)}")
            for series, values, is_cold in (
                (f"{tool} pull (cold)", cold, True),
                (f"{tool} pull (warm)", warm_pull, False),
                (f"{tool} push (warm)", warm_push, False),
            ):
                if values:
                    rows.append({
                        "group": name,
                        "series": series,
                        "elapsed_s": values[0] if is_cold else statistics.median(values),
                    })

    warnings = [s for s in samples if s.verified != "ok"]
    if warnings:
        print(f"\n{len(warnings)} of {len(samples)} replays had verification warnings")

    chart = grouped_chart(
        pd.DataFrame(rows), SERIES, "rustc-josh-sync replays through josh (lower is better)"
    )
    print(f"chart written to {save_chart(chart, OUTPUT_DIR / 'rustc_josh_sync.png')}")
