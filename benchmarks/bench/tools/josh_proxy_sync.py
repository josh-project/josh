"""Replay one historical rustc-josh-sync pull or push through josh-proxy.

Only the single git command that exercises josh is timed: the ``git fetch`` of
the filtered history (pull) or the ``git push`` that josh reverse-filters onto
rust-lang/rust (push). Local bookkeeping (checkouts, the merge commit) and the
verification against the historical result are untimed.
"""

import shlex
from dataclasses import dataclass, field
from pathlib import Path

from bench.preparation.rustc_josh_sync.config import UPSTREAM_REPO
from bench.proxy import JoshProxy
from bench.shell import run
from bench.timing import Timer


@dataclass
class SyncSample:
    """One replayed sync: what ran it and how long the josh step took."""

    tool: str  # "proxy" | "cli"
    subtree: str
    direction: str  # "pull" | "push"
    cache: str  # "cold" | "warm"
    elapsed: float
    event: dict = field(repr=False)
    verified: str = "ok"  # "ok" | warning text


def _git(repo: Path, args: str) -> str:
    return run(f"git {args}", cwd=str(repo))


class VerificationError(RuntimeError):
    pass


def replay_pull(
    proxy: JoshProxy, work: Path, filter_spec: str, ev: dict
) -> tuple[float, str]:
    """Replay a pull; return (elapsed seconds of the fetch, verification status).

    The historical "Prepare for merging" commit is reused as-is (it lives in the
    filtered history the work clone shares), so the replayed merge must
    reproduce the historical merge tree exactly.
    """
    _git(work, f"checkout -q -B replay {ev['hist_prepare']}")
    _git(work, "clean -fdq")

    url = proxy.url(UPSTREAM_REPO, filter_spec, at=ev["pulled_rust_sha"])
    with Timer() as t:
        _git(work, f"fetch -q {shlex.quote(url)}")

    fetched = _git(work, "rev-parse FETCH_HEAD").strip()
    if fetched != ev["hist_fetch_head"]:
        raise VerificationError(
            f"pull {ev['hist_merge'][:12]}: proxy served {fetched[:12]}, history has "
            f"{ev['hist_fetch_head'][:12]} -- josh version/filter drift?"
        )

    verified = "ok"
    msg = shlex.quote(ev["merge_msg"] or "Merge from rust-lang/rust")
    try:
        _git(work, f"merge -q --no-ff --no-verify -m {msg} FETCH_HEAD")
    except RuntimeError:
        # The historical merge had manual conflict resolutions; the fetch (the
        # timed josh work) already succeeded, so keep the sample.
        _git(work, "merge --abort")
        verified = "merge conflict (historical resolution not reproducible)"
    else:
        tree = _git(work, "rev-parse HEAD^{tree}").strip()
        if tree != ev["hist_merge_tree"]:
            verified = f"merge tree {tree[:12]} != historical {ev['hist_merge_tree'][:12]}"
    return t.elapsed, verified


def replay_push(
    proxy: JoshProxy, work: Path, upstream: Path, filter_spec: str, ev: dict, branch: str
) -> tuple[float, str]:
    """Replay a push; return (elapsed seconds of the push, verification status).

    The push branch is created on the upstream mirror at the rust base the real
    sync used (rustc-josh-sync pushes that base to the user's fork first), then
    the historical subtree head is pushed through the proxy, which
    reverse-filters it onto that base.
    """
    _git(upstream, f"update-ref refs/heads/{branch} {ev['base_rust_sha']}")

    url = proxy.url(UPSTREAM_REPO, filter_spec)
    with Timer() as t:
        _git(work, f"push -q {shlex.quote(url)} {ev['subtree_head']}:refs/heads/{branch}")

    verified = "ok"
    pushed_tree = _git(upstream, f"rev-parse refs/heads/{branch}^{{tree}}").strip()
    if pushed_tree != ev["hist_pr_tree"]:
        verified = (
            f"pushed rust tree {pushed_tree[:12]} != historical PR head tree "
            f"{ev['hist_pr_tree'][:12]}"
        )
    else:
        # Roundtrip check like rustc-josh-sync's: the filtered view of the
        # branch josh just created must be exactly the head we pushed.
        out = _git(work, f"ls-remote {shlex.quote(url)} refs/heads/{branch}")
        roundtrip = out.split()[0] if out.split() else "<missing>"
        if roundtrip != ev["subtree_head"]:
            verified = f"roundtrip {roundtrip[:12]} != pushed {ev['subtree_head'][:12]}"
    return t.elapsed, verified
