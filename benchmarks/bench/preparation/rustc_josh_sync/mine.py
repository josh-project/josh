"""Mine real historical sync events out of rust-lang/rust history.

Both directions of the rustc-josh-sync workflow leave durable traces:

- A *pull* records itself in the subtree repo as a "Prepare for merging" commit
  (writing the pulled rust SHA into ``rust-version``) followed by a merge of the
  filtered rust history. Those commits get pushed back to rust-lang/rust on the
  next push, so they are visible in our filtered history. Detection goes by the
  ``rust-version`` content change, not the message, so older pre-josh-sync
  "Rustup" pulls (miri) are found too.
- A *push* lands in rust-lang/rust as a bors merge of the reverse-filtered
  subtree branch: a merge on main whose diff is confined to the subtree path and
  bumps ``<path>/rust-version``. Its second parent is the PR head produced by
  josh, and the ``rust-version`` blob in it names the rust base the push was
  built on.

The result is a manifest (cached as JSON under ``target/rustc-josh-sync/``)
listing, per subtree, the most recent `depth` events per direction in replay
order: pulls oldest to newest (the oldest is the cold sample -- a full-history
pull, comparable across subtrees), then pushes oldest to newest.
"""

import json
import re
import subprocess
from pathlib import Path

from bench.preparation.rustc_josh_sync.config import RUST_REVISION
from bench.shell import run

_SHA40 = re.compile(r"^[0-9a-f]{40}$")


def _git(repo: Path, args: str) -> str:
    return run(f"git {args}", cwd=str(repo))


def _object_exists(repo: Path, obj: str) -> bool:
    return (
        subprocess.run(
            ["git", "-C", str(repo), "cat-file", "-e", obj], capture_output=True
        ).returncode
        == 0
    )


def mine_pulls(filtered: Path, source: Path, depth: int) -> list[dict]:
    """Find historical pulls in the filtered (subtree-side) history, newest first.

    Prepare commits and their pull merges live in the second-parent subgraphs of
    the filtered history (they reached rust-lang/rust inside push PR branches),
    so the whole DAG is walked, not just the first-parent chain.
    """
    parents: dict[str, list[str]] = {}
    children: dict[str, list[str]] = {}
    for line in _git(filtered, "rev-list --parents refs/heads/filtered").splitlines():
        row = line.split()
        parents[row[0]] = row[1:]
        for p in row[1:]:
            children.setdefault(p, []).append(row[0])

    # All non-merge commits (anywhere in the DAG) that changed rust-version,
    # newest first; prepare commits are the ones setting it to a commit SHA.
    candidates = _git(
        filtered,
        "log --full-history --no-merges --format=%H refs/heads/filtered -- rust-version",
    ).split()

    events: list[dict] = []
    seen: set[str] = set()
    for c in candidates:
        if len(events) >= depth:
            break
        if len(parents[c]) != 1:
            continue
        pulled = _git(filtered, f"show {c}:rust-version").strip()
        if not _SHA40.match(pulled):
            continue
        # The pull merge is the merge child whose first parent is the prepare
        # commit; it keeps the prepare commit's rust-version.
        merge = next(
            (
                h
                for h in children.get(c, [])
                if len(parents[h]) >= 2 and parents[h][0] == c
            ),
            None,
        )
        if merge is None or merge in seen:
            continue
        if _git(filtered, f"show {merge}:rust-version").strip() != pulled:
            continue
        if not _object_exists(source, pulled):  # pulled rust commit not in the mirror
            continue
        seen.add(merge)
        events.append({
            "kind": "pull",
            "ts": int(_git(filtered, f"log -1 --format=%ct {merge}").strip()),
            "hist_prepare": c,
            "base_subtree": parents[c][0],
            "pulled_rust_sha": pulled,
            "hist_merge": merge,
            "hist_merge_tree": _git(filtered, f"rev-parse {merge}^{{tree}}").strip(),
            "hist_fetch_head": parents[merge][1],
            "merge_msg": _git(filtered, f"log -1 --format=%s {merge}").strip(),
        })
    return events


def _confined_sync_merge(source: Path, merge: str, path: str) -> bool:
    """True if `merge` brings only changes under `path`, including rust-version."""
    names = _git(source, f"diff --name-only {merge}^1 {merge}").splitlines()
    return (
        bool(names)
        and all(n.startswith(f"{path}/") for n in names)
        and f"{path}/rust-version" in names
    )


def mine_pushes(
    source: Path, filtered: Path, josh_filter: Path, filter_spec: str, path: str, depth: int
) -> list[dict]:
    """Find historical pushes for subtree `path` on rust-lang/rust main, newest first.

    A push lands either as a direct bors merge of the subtree-update PR, or --
    more commonly -- inside a rollup, as a "Rollup merge of #NNN" commit on the
    rollup branch. ``-m`` diffs merges against their first parent, so the outer
    walk finds every main-branch merge that bumped ``rust-version``; if that
    merge is not itself confined to the subtree it is a rollup, and the actual
    subtree-update merge is looked up on the rollup branch the same way.
    """
    outers = _git(
        source,
        "log --first-parent -m --merges --format='%H %ct'"
        f" refs/heads/main -- {path}/rust-version",
    ).splitlines()

    events: list[dict] = []
    seen: set[str] = set()
    for line in outers:
        if len(events) >= depth:
            break
        outer, ts = line.split()
        merge = outer
        if not _confined_sync_merge(source, merge, path):
            inner = _git(
                source,
                f"log -n 1 --first-parent -m --merges --format=%H {outer}^2"
                f" -- {path}/rust-version",
            ).strip()
            if not inner or not _confined_sync_merge(source, inner, path):
                continue
            merge = inner
            # The inner merge must be the one responsible for the outer bump.
            if _git(source, f"show {merge}:{path}/rust-version") != _git(
                source, f"show {outer}:{path}/rust-version"
            ):
                continue
        if merge in seen:
            continue
        seen.add(merge)
        base = _git(source, f"show {merge}^2:{path}/rust-version").strip()
        if not _SHA40.match(base) or not _object_exists(source, base):
            continue
        pr_head = _git(source, f"rev-parse {merge}^2").strip()

        # The subtree-side branch head that was pushed = the filtered image of
        # the PR head. Incremental thanks to josh-filter's persistent cache in
        # the filtered repo (only the PR-side commits are new).
        _git(filtered, f"update-ref refs/mining/tmp {pr_head}")
        run(
            f"{josh_filter} -s '{filter_spec}' refs/mining/tmp --update refs/mining/out",
            cwd=str(filtered),
        )
        events.append({
            "kind": "push",
            "ts": int(ts),
            "hist_rust_merge": merge,
            "hist_pr_head": pr_head,
            "hist_pr_tree": _git(source, f"rev-parse {pr_head}^{{tree}}").strip(),
            "base_rust_sha": base,
            "subtree_head": _git(filtered, "rev-parse refs/mining/out").strip(),
        })
    return events


def _assemble(pulls: list[dict], pushes: list[dict]) -> list[dict]:
    """Order events for replay: all pulls oldest to newest, then all pushes.

    The oldest pull comes first so the cold sample is a full-history pull for
    every subtree. Pushes follow against the then-warm cache, which matches
    production: sync cadences differ per direction (miri pulls daily but pushes
    rarely), and whenever a push happens the proxy has long been warmed by the
    pulls in between. Replays are independent (each event carries its own base
    refs), so cross-direction calendar order does not matter.
    """
    return sorted(pulls, key=lambda e: e["ts"]) + sorted(pushes, key=lambda e: e["ts"])


def mine_subtree(
    name: str,
    source: Path,
    filtered: Path,
    filter_spec: str,
    path: str,
    josh_filter: Path,
    josh_commit: str,
    depth: int,
    target_dir: Path,
) -> dict:
    """Build (or load the cached) replay manifest for one subtree.

    Cached per subtree, keyed by the rust pin, the josh commit (it produced the
    filtered history the events reference) and the depth.
    """
    cache = (
        target_dir
        / "rustc-josh-sync"
        / f"manifest-{name}-{RUST_REVISION[:12]}-{josh_commit[:12]}-d{depth}.json"
    )
    if cache.exists():
        return json.loads(cache.read_text())

    pulls = mine_pulls(filtered, source, depth)
    pushes = mine_pushes(source, filtered, josh_filter, filter_spec, path, depth)
    events = _assemble(pulls, pushes)
    counts = {
        "pull": sum(e["kind"] == "pull" for e in events),
        "push": sum(e["kind"] == "push" for e in events),
    }
    for kind in ("pull", "push"):
        if counts[kind] < depth:
            print(f"warning: {name}: only {counts[kind]} {kind} events (wanted {depth})")
    print(f"mined {name}: {counts['pull']} pulls, {counts['push']} pushes")

    manifest = {
        "rust_revision": RUST_REVISION,
        "josh_commit": josh_commit,
        "depth": depth,
        "filter": filter_spec,
        "path": path,
        "counts": counts,
        "events": events,
    }
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(json.dumps(manifest, indent=2))
    return manifest
