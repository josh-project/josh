#!/usr/bin/env bash
# Functional test for the josh CLI on a platform where `josh compose` cannot
# run. Exercises filtering, cloning, pulling and pushing against a local bare
# repository: no server, no network, nothing but git.
#
#   tests/windows/cli.sh <dir-containing-josh-and-josh-filter>
#
# The clone deliberately targets a relative directory, which is what turns a
# path into a remote URL internally — the case that was broken on Windows.
set -euo pipefail

BIN_DIR="$(cd "$1" && pwd)"
EXE=""
[ -f "$BIN_DIR/josh.exe" ] && EXE=".exe"
JOSH="$BIN_DIR/josh$EXE"
JOSH_FILTER="$BIN_DIR/josh-filter$EXE"
[ -f "$JOSH" ] || { echo "FAIL: $JOSH not found" >&2; exit 1; }
[ -f "$JOSH_FILTER" ] || { echo "FAIL: $JOSH_FILTER not found" >&2; exit 1; }

WORK="$(mktemp -d)"
BRANCH="main"
fail() { echo "FAIL: $*" >&2; exit 1; }

echo "== setup: local upstream"
git init -q --bare -b "$BRANCH" "$WORK/upstream.git"
git init -q -b "$BRANCH" "$WORK/seed"
git -C "$WORK/seed" config user.email t@t
git -C "$WORK/seed" config user.name t
echo hello > "$WORK/seed/README.md"
git -C "$WORK/seed" add . && git -C "$WORK/seed" commit -qm "c1: readme"
mkdir -p "$WORK/seed/src" && echo lib > "$WORK/seed/src/lib.txt"
git -C "$WORK/seed" add . && git -C "$WORK/seed" commit -qm "c2: lib"
git -C "$WORK/seed" push -q "$WORK/upstream.git" "$BRANCH"

echo "== josh-filter"
git clone -q "$WORK/upstream.git" "$WORK/plain"
(cd "$WORK/plain" && "$JOSH_FILTER" ":prefix=lib" "$BRANCH") || fail "josh-filter"
git -C "$WORK/plain" ls-tree --name-only -r FILTERED_HEAD | grep -qx "lib/README.md" \
  || fail "josh-filter: prefix missing from FILTERED_HEAD"
[ "$(git -C "$WORK/plain" rev-list --count FILTERED_HEAD)" = 2 ] \
  || fail "josh-filter: expected 2 commits"

echo "== josh clone, into a relative directory"
mkdir -p "$WORK/cli" && cd "$WORK/cli"
"$JOSH" clone "$WORK/upstream.git" ":prefix=lib" ./clone || fail "josh clone"
[ -f "$WORK/cli/clone/lib/README.md" ] || fail "josh clone: prefix missing"
[ "$(git -C "$WORK/cli/clone" rev-list --count HEAD)" = 2 ] \
  || fail "josh clone: expected 2 commits"

echo "== josh changes pull"
echo more >> "$WORK/seed/src/lib.txt"
git -C "$WORK/seed" commit -qam "c3: more lib"
C3="$(git -C "$WORK/seed" rev-parse HEAD)"
git -C "$WORK/seed" push -q "$WORK/upstream.git" "$BRANCH"
(cd "$WORK/cli/clone" && "$JOSH" changes pull) || fail "josh changes pull"
grep -q more "$WORK/cli/clone/lib/src/lib.txt" \
  || fail "josh changes pull: upstream change did not arrive through the filter"

echo "== josh push"
cd "$WORK/cli/clone"
git config user.email t@t
git config user.name t
echo change >> lib/src/lib.txt
git commit -qam "c4: change through the filter"
"$JOSH" push origin "HEAD:refs/heads/roundtrip" --base "$BRANCH" || fail "josh push"
RT="$(git -C "$WORK/upstream.git" rev-parse refs/heads/roundtrip)" \
  || fail "josh push: branch missing upstream"
git -C "$WORK/upstream.git" show "$RT:src/lib.txt" | grep -q change \
  || fail "josh push: change not reverse-filtered to src/lib.txt"
[ "$(git -C "$WORK/upstream.git" rev-parse "$RT^")" = "$C3" ] \
  || fail "josh push: pushed commit is not rooted on the upstream tip"

echo "PASS"
