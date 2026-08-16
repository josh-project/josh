"""Replay historical rustc-josh-sync pulls and pushes with the josh CLI.

Same events as :mod:`bench.tools.josh_proxy_sync`, but instead of a josh-proxy
between the developer and the upstream, the subtree checkout uses the josh CLI
directly against the (unfiltered) upstream: ``josh fetch`` fetches the raw
history and filters it locally, ``josh push`` reverse-filters locally and
pushes the reconstructed rust commits.

The CLI applies the configured remote filter including its semantic
``:~(history=...,gpgsig=...)`` meta args (fixed in "Apply semantic meta args
of remote filters in the josh CLI"; older versions dropped them and cannot
run this pass -- their SHA-exact verification fails on the first pull), so
with the production compat filter its output is SHA-identical to the
proxy/josh-filter reference and both pulls and pushes replay with SHA-exact
verification.

Timing mirrors the proxy runner: the josh command(s) doing the sync work are
timed, local bookkeeping (branch setup, the merge commit) and verification are
not. For pulls that is ``josh fetch``; a cold fetch transfers the full rust
history and filters it from scratch (the CLI's caches live in the work repo,
which starts fresh). For pushes it is ``josh fetch`` of the base branch plus
``josh push`` -- the CLI needs the raw base locally for reverse filtering,
work the proxy performs server-side inside its timed push.
"""

import shlex
from pathlib import Path

from bench.shell import run
from bench.timing import Timer


def _git(repo: Path, args: str) -> str:
    return run(f"git {args}", cwd=str(repo))


def setup_cli_remote(
    josh: str | Path, work: Path, upstream_url: str, filter_spec: str,
    remote: str = "upstream",
) -> None:
    """Configure the josh remote for the upstream mirror in the work clone."""
    run(
        f"{josh} remote add {remote} {shlex.quote(upstream_url)} {shlex.quote(filter_spec)}",
        cwd=str(work),
    )


def replay_pull_cli(
    josh: str | Path, work: Path, upstream: Path, ev: dict, branch: str
) -> tuple[float, str]:
    """Replay a pull with the josh CLI; return (elapsed, verification status).

    The upstream state at the historical pull is presented as `branch` in the
    mirror; the historical "Prepare for merging" commit (plain-filter dialect)
    is reused as-is, so the replayed merge must reproduce the historical tree.
    """
    _git(upstream, f"update-ref refs/heads/{branch} {ev['pulled_rust_sha']}")
    _git(work, f"checkout -q -B replay {ev['hist_prepare']}")
    _git(work, "clean -fdq")

    with Timer() as t:
        run(f"{josh} fetch -r upstream -R {branch}", cwd=str(work))

    fetched = _git(work, f"rev-parse refs/remotes/upstream/{branch}").strip()
    if fetched != ev["hist_fetch_head"]:
        raise RuntimeError(
            f"cli pull {ev['hist_merge'][:12]}: josh fetch produced {fetched[:12]}, "
            f"reference has {ev['hist_fetch_head'][:12]}"
        )

    verified = "ok"
    msg = shlex.quote(ev["merge_msg"] or "Merge from rust-lang/rust")
    try:
        _git(work, f"merge -q --no-ff --no-verify -m {msg} {fetched}")
    except RuntimeError:
        _git(work, "merge --abort")
        verified = "merge conflict (historical resolution not reproducible)"
    else:
        tree = _git(work, "rev-parse HEAD^{tree}").strip()
        if tree != ev["hist_merge_tree"]:
            verified = f"merge tree {tree[:12]} != historical {ev['hist_merge_tree'][:12]}"
    return t.elapsed, verified


def replay_push_cli(
    josh: str | Path, work: Path, upstream: Path, ev: dict, branch: str
) -> tuple[float, str]:
    """Replay a push with the josh CLI; return (elapsed, verification status)."""
    _git(upstream, f"update-ref refs/heads/{branch} {ev['base_rust_sha']}")
    # josh push takes a local ref, not a raw SHA.
    _git(work, f"branch -f cli-push {ev['subtree_head']}")

    with Timer() as t:
        run(f"{josh} fetch -r upstream -R {branch}", cwd=str(work))
        run(f"{josh} push upstream cli-push:refs/heads/{branch}", cwd=str(work))

    verified = "ok"
    pushed_tree = _git(upstream, f"rev-parse refs/heads/{branch}^{{tree}}").strip()
    if pushed_tree != ev["hist_pr_tree"]:
        verified = (
            f"pushed rust tree {pushed_tree[:12]} != historical PR head tree "
            f"{ev['hist_pr_tree'][:12]}"
        )
    else:
        # Roundtrip like rustc-josh-sync's: filtering the branch josh just
        # created must give back exactly the head we pushed.
        run(f"{josh} fetch -r upstream -R {branch}", cwd=str(work))
        roundtrip = _git(work, f"rev-parse refs/remotes/upstream/{branch}").strip()
        if roundtrip != ev["subtree_head"]:
            verified = f"roundtrip {roundtrip[:12]} != pushed {ev['subtree_head'][:12]}"
    return t.elapsed, verified
