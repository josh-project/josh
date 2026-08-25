#!/usr/bin/env bash
# Functional test for josh-proxy on a platform where `josh compose` cannot run.
# Drives a real proxy against a real upstream: filtered clone, pinned-SHA fetch,
# reverse-filter push, and reuse of the --local cache across a restart.
#
#   UPSTREAM_URL=http://127.0.0.1:8177 tests/windows/proxy.sh <josh-proxy> [cache-dir]
#
# UPSTREAM_URL is the base URL of a git server exporting the repositories in
# UPSTREAM_ROOT (tests/windows/serve-git.ps1 provides one on Windows). The
# optional cache directory lets a caller exercise unusual path forms.
set -euo pipefail

JOSH_PROXY="$1"
UPSTREAM_ROOT="${UPSTREAM_ROOT:?set UPSTREAM_ROOT to the served directory}"
UPSTREAM_URL="${UPSTREAM_URL:?set UPSTREAM_URL to the serving base URL}"
WORK="$(mktemp -d)"
LOCAL_DIR="${2:-$WORK/local}"
PORT="${JOSH_PORT:-42190}"
BRANCH="main"

JOSH_PID=""
cleanup() { [ -n "$JOSH_PID" ] && kill "$JOSH_PID" 2>/dev/null || true; }
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  [ -s "$WORK/josh.log" ] && { echo "--- josh-proxy log:" >&2; cat "$WORK/josh.log" >&2; }
  exit 1
}

start_proxy() {
  "$JOSH_PROXY" --local "$LOCAL_DIR" --remote "$UPSTREAM_URL" \
    "--port=$PORT" --no-background >>"$WORK/josh.log" 2>&1 &
  JOSH_PID=$!
  for _ in $(seq 1 100); do
    curl -s -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/" && return 0
    [ "$?" -ne 7 ] && return 0
    sleep 0.1
  done
  fail "josh-proxy did not start"
}

stop_proxy() {
  kill "$JOSH_PID" 2>/dev/null || true
  for _ in $(seq 1 20); do kill -0 "$JOSH_PID" 2>/dev/null || break; sleep 0.1; done
  kill -0 "$JOSH_PID" 2>/dev/null && fail "josh-proxy did not exit when terminated"
  JOSH_PID=""
}

echo "== setup: upstream repository"
rm -rf "$UPSTREAM_ROOT/upstream.git"
git init -q --bare -b "$BRANCH" "$UPSTREAM_ROOT/upstream.git"
git -C "$UPSTREAM_ROOT/upstream.git" config http.receivepack true
git init -q -b "$BRANCH" "$WORK/seed"
git -C "$WORK/seed" config user.email t@t
git -C "$WORK/seed" config user.name t
echo hello > "$WORK/seed/README.md"
git -C "$WORK/seed" add . && git -C "$WORK/seed" commit -qm "c1: readme"
C1="$(git -C "$WORK/seed" rev-parse HEAD)"
mkdir -p "$WORK/seed/src" && echo lib > "$WORK/seed/src/lib.txt"
git -C "$WORK/seed" add . && git -C "$WORK/seed" commit -qm "c2: lib"
C2="$(git -C "$WORK/seed" rev-parse HEAD)"
git -C "$WORK/seed" push -q "$UPSTREAM_ROOT/upstream.git" "$BRANCH"

echo "== boot"
start_proxy
FILTERED="http://127.0.0.1:$PORT/upstream.git:prefix=lib.git"

echo "== filtered clone"
git clone -q "$FILTERED" "$WORK/clone" || fail "filtered clone"
[ -f "$WORK/clone/lib/README.md" ] || fail "prefix missing from the clone"
[ "$(git -C "$WORK/clone" rev-list --count HEAD)" = 2 ] || fail "expected 2 commits"

echo "== pinned-SHA fetch"
# The filter separator is also exercised percent-encoded, as clients send it.
git -C "$WORK/clone" fetch -q "http://127.0.0.1:$PORT/upstream.git@$C1%3Aprefix=lib.git" HEAD \
  || fail "pinned fetch"
git -C "$WORK/clone" ls-tree --name-only -r FETCH_HEAD | grep -qx "lib/README.md" \
  || fail "pinned fetch: README missing"
git -C "$WORK/clone" ls-tree --name-only -r FETCH_HEAD | grep -q "lib/src" \
  && fail "pinned fetch resolved past the pinned commit"

echo "== reverse push"
git -C "$WORK/clone" config user.email t@t
git -C "$WORK/clone" config user.name t
echo change >> "$WORK/clone/lib/src/lib.txt"
git -C "$WORK/clone" commit -qam "c3: change through the filter"
git -C "$WORK/clone" push -q -o "base=refs/heads/$BRANCH" origin HEAD:refs/heads/roundtrip \
  || fail "reverse push"
RT="$(git -C "$UPSTREAM_ROOT/upstream.git" rev-parse refs/heads/roundtrip)" \
  || fail "reverse push: branch missing upstream"
git -C "$UPSTREAM_ROOT/upstream.git" show "$RT:src/lib.txt" | grep -q change \
  || fail "reverse push: change not reverse-filtered to src/lib.txt"
[ "$(git -C "$UPSTREAM_ROOT/upstream.git" rev-parse "$RT^")" = "$C2" ] \
  || fail "reverse push: pushed commit is not rooted on the upstream tip"

echo "== cache reuse across a restart"
# Consumers run one proxy per operation rather than a daemon, so the --local
# cache has to survive a clean stop and serve the next instance.
stop_proxy
start_proxy
git -C "$WORK/clone" fetch -q origin || fail "fetch against the reused cache"

stop_proxy
echo "PASS"
