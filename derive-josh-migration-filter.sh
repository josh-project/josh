#!/usr/bin/env bash
# Derive the josh-sync migration filters for rust-lang/rust subtrees that are synced
# with git-subtree-style merges. Prints, per subtree:
#
#   :~(history="keep-trivial-merges,no-splice")[:rev(<=TIP:prefix=DIR,<=LANDING:SQUASH)]:/DIR
#
#   TIP     = last standalone-repo commit merged into rust. Its ancestry (the whole
#             shared subtree lineage) is reproduced SHA-identically, so the subtree repo
#             keeps a merge base with the filtered history and pushes keep working.
#   LANDING = the rust commit that landed the sync merge bringing TIP in. Rust-native
#             history <=LANDING (but not <=TIP) is squashed away instead of minted, which
#             removes the mass commit duplication otherwise seen on the initial josh-sync
#             pull.
#
# Commits in the squash range whose changes are NOT part of TIP are whitelisted with
# additional ==sha rev entries so they survive the squash:
#   - standalone-layout side commits on the sync branch (TIP..sync) via ==sha:prefix=DIR
#     (reproduced SHA-identically, like the TIP lineage)
#   - rust-layout fixups on the sync PR branch (sync..LANDING) that touch DIR via ==sha:/
#     (minted: new SHA, but message/author/diff preserved as their own commit)
# A remaining delta can survive only in tree form: rust-side changes to DIR that were
# never upstreamed and got preserved by the sync merge's resolution -- their original
# commits are squashed (that is the point of the squash). This delta folds into the
# first post-LANDING filtered commit; the filtered(LANDING) check below verifies nothing is
# silently lost.
#
# Each derived TIP and LANDING is also written to refs/heads/subtree-migration/<name>/{TIP,LANDING}
# for inspection (in single-subtree mode <name> is the basename of the directory), and
# the derived filter is applied with josh-filter to produce the resulting subtree
# history under refs/heads/subtree-migration/<name>/filtered (skipped in partial
# clones, where josh-filter would fetch all trees). The filtered tip tree is compared
# against the subtree directory in rust as an extra check.
#
# The filter is additionally applied to LANDING itself (-> .../<name>/filtered-at-LANDING): the
# result must be TIP plus exactly the whitelisted commits, and content-complete at LANDING.
# If DIR's content at LANDING still differs (never-upstreamed rust-side changes whose
# original commits are squashed), that is fine as long as post-LANDING commits exist for it
# to fold into -- otherwise the subtree FAILS, because the delta would be silently lost.
#
# Both SHAs are derived from the commit graphs alone (merge base + landing merge).
# Both history flags and the entry order are required for correctness. The `:SQUASH`
# rev-entry semantics require josh release r26.07.28 or newer.
#
# Usage:
#   derive-josh-migration-filter.sh [-C <rust-clone>] [--at <date|committish>] [--no-fetch]
#
# Run inside (or point -C at) any clone of rust-lang/rust -- bare or non-bare, full or
# partial (tree:0 partial clones keep everything small and fast). With no further
# arguments it fetches what it needs by itself (rust from origin, each remaining
# subtree repo's head into refs/subtrees/<name>, using --filter=tree:0 when the clone
# is partial), writes a commit-graph for fast graph walks, and derives the filters for
# all remaining git-subtree directories:
#   clippy, rustc_codegen_cranelift, rustc_codegen_gcc, rustfmt, portable-simd
#
# A single subtree (also ones not in the builtin list) can be derived with:
#   derive-josh-migration-filter.sh -C <rust-clone> --dir <subtree-dir> --standalone <ref>
# (no fetching happens in this mode; the ref must already exist)
#
#   -C <path>              rust clone (default .)
#   --dir <path>           single-subtree mode: subtree directory in rust
#   --standalone <ref>     single-subtree mode: ref of the standalone repo's main branch
#   --rust-ref <ref>       default: auto-detect (origin/main, main, origin/master, master)
#   --at <date|committish> derive as of that point in time instead of now
#   --no-fetch             skip all fetching, use the refs as they are
#   --history-flags <s>    default keep-trivial-merges,no-splice (both are required)
#   --josh-filter <bin>    josh-filter binary to apply the filter with (default:
#                          josh-filter from PATH; needs josh >= r26.07.28 for :SQUASH)
#   --no-apply             don't apply the filter / don't write the filtered ref
#
# Exit codes: 0 ok, 1 one or more subtrees failed, 2 usage,
# 3 not a git-subtree repo (use plain ':/DIR' filter), 4 ambiguous graph.
#
# Before freezing a filter: re-run right before the migration (a new sync moves both
# SHAs). In a full clone the post-LANDING path check runs automatically; in partial clones
# verify manually: git log <LANDING>..<rust-ref> -- <DIR>

set -euo pipefail

SUBTREES="\
clippy src/tools/clippy https://github.com/rust-lang/rust-clippy.git
cranelift compiler/rustc_codegen_cranelift https://github.com/rust-lang/rustc_codegen_cranelift.git
gcc compiler/rustc_codegen_gcc https://github.com/rust-lang/rustc_codegen_gcc.git
rustfmt src/tools/rustfmt https://github.com/rust-lang/rustfmt.git
portable-simd library/portable-simd https://github.com/rust-lang/portable-simd.git"

RUST_REPO=.
RUST_REF=
DIR=
STANDALONE=
AT=
FETCH=1
HISTORY_FLAGS='keep-trivial-merges,no-splice'
JOSH_FILTER=${JOSH_FILTER:-josh-filter}
APPLY=1
UNCLEAN=

usage() { grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 2; }

while [ $# -gt 0 ]; do
    case "$1" in
        -C) RUST_REPO=$2; shift 2 ;;
        --dir) DIR=$2; shift 2 ;;
        --standalone) STANDALONE=$2; shift 2 ;;
        --rust-ref) RUST_REF=$2; shift 2 ;;
        --at) AT=$2; shift 2 ;;
        --no-fetch) FETCH=0; shift ;;
        --history-flags) HISTORY_FLAGS=$2; shift 2 ;;
        --josh-filter) JOSH_FILTER=$2; shift 2 ;;
        --no-apply) APPLY=0; shift ;;
        *) usage ;;
    esac
done

g() { git -C "$RUST_REPO" "$@"; }

# --- setup ------------------------------------------------------------------------------
detect_rust_ref() {
    for r in refs/remotes/origin/main refs/heads/main refs/remotes/origin/master refs/heads/master; do
        if g rev-parse --verify --quiet "$r^{commit}" >/dev/null; then echo "$r"; return; fi
    done
    echo "error: no main/master ref found in $RUST_REPO -- is this a rust-lang/rust clone?" >&2
    exit 2
}

fetch_filter_arg() {
    # --filter fetches only work when the clone is configured for partial clone.
    if [ -n "$(g config --get extensions.partialclone 2>/dev/null)" ] \
        || [ "$(g config --get remote.origin.promisor 2>/dev/null)" = "true" ]; then
        echo "--filter=tree:0"
    fi
}

setup() {
    local farg
    farg=$(fetch_filter_arg)
    if [ "$FETCH" = 1 ]; then
        if g remote get-url origin >/dev/null 2>&1; then
            echo "fetching rust from origin..." >&2
            g fetch -q origin || echo "warning: fetching origin failed, using existing refs" >&2
        fi
        while read -r name dir url; do
            echo "fetching $name..." >&2
            # shellcheck disable=SC2086
            g fetch -q $farg "$url" "+HEAD:refs/subtrees/$name" \
                || echo "warning: fetching $name failed, using existing refs/subtrees/$name" >&2
        done <<< "$SUBTREES"
    fi
    echo "updating commit-graph (speeds up the graph walks)..." >&2
    g commit-graph write --reachable 2>/dev/null || true
}

# --- resolve the two branch states at the requested point in time -----------------------
resolve_at() { # $1 = ref
    if [ -z "$AT" ]; then
        g rev-parse --verify "$1^{commit}"
    elif g rev-parse --verify --quiet "$AT^{commit}" >/dev/null; then
        # --at is a committish: use it verbatim for the rust side, time-resolve standalone
        if [ "$1" = "$RUST_REF" ]; then
            g rev-parse --verify "$AT^{commit}"
        else
            local t; t=$(g log -1 --format=%cI "$AT")
            g rev-list -1 --first-parent --before="$t" "$1"
        fi
    else
        # --first-parent: stay on the branch spine; a plain walk can surface
        # date-skewed commits from merged side histories.
        g rev-list -1 --first-parent --before="$AT" "$1"
    fi
}

# --- apply the derived filter, write the filtered ref ------------------------------------
apply_filter() { # $1=NAME $2=filter $3=rust commit $4=DIR $5=TIP $6=LANDING $7=post-LANDING count
    local NAME=$1 FILTER=$2 MASTER=$3 DIR=$4 TIP=$5 LANDING=$6 POSTN=${7:-0}
    local ref="refs/heads/subtree-migration/$NAME/filtered"
    local rref="refs/heads/subtree-migration/$NAME/filtered-at-LANDING"
    [ "$APPLY" = 1 ] || return 0
    if ! command -v "$JOSH_FILTER" >/dev/null 2>&1; then
        echo "filtered:   SKIPPED ($JOSH_FILTER not found; pass --josh-filter <bin>)"
        return 0
    fi
    if [ -n "$(fetch_filter_arg)" ]; then
        echo "filtered:   SKIPPED (partial clone; josh-filter would fetch all trees)"
        return 0
    fi
    echo "applying filter with $JOSH_FILTER..." >&2
    local OUT_TMP rc=0
    OUT_TMP=$(mktemp)
    # josh-filter operates on the repo in its cwd
    (cd "$RUST_REPO" && "$JOSH_FILTER" --update "$ref" "$FILTER" "$MASTER" \
                     && "$JOSH_FILTER" --update "$rref" "$FILTER" "$LANDING") \
        >"$OUT_TMP" 2>&1 || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "filtered:   FAILED ($JOSH_FILTER exited $rc; josh >= r26.07.28 needed for :SQUASH)"
        sed 's/^/            /' "$OUT_TMP" | tail -5
        rm -f "$OUT_TMP"
        return 0
    fi
    rm -f "$OUT_TMP"
    local match
    if [ "$(g rev-parse "$ref^{tree}")" = "$(g rev-parse "$MASTER:$DIR")" ]; then
        match="tree matches $DIR in rust"
    else
        match="tree DIFFERS from $DIR in rust -- inspect!"
    fi
    echo "filtered:   $ref"
    echo "            tip $(g rev-parse --short "$ref"), $match"

    # filtered(LANDING) check: the filter applied to LANDING must give TIP plus exactly the
    # whitelisted commits; any remaining DIR delta at LANDING (never-upstreamed rust-side
    # changes) needs post-LANDING commits to fold into, else it is lost.
    local FR desc stat
    FR=$(g rev-parse "$rref")
    if [ "$FR" = "$TIP" ]; then
        desc="TIP exactly"
    else
        if ! g merge-base --is-ancestor "$TIP" "$FR"; then
            echo "filter(LANDING): FAIL -- TIP is not an ancestor of filtered(LANDING) ($rref)"
            return 1
        fi
        desc="TIP + $(g rev-list --count "$TIP..$FR") whitelisted commit(s)"
    fi
    echo "filter(LANDING):"
    echo "            graph:   $desc ($rref)"
    if [ "$(g rev-parse "$FR^{tree}")" = "$(g rev-parse "$LANDING:$DIR")" ]; then
        echo "            content: matches DIR@LANDING -- complete"
    else
        stat=$(g diff --shortstat "$FR" "$LANDING:$DIR" | sed 's/^ //')
        if [ "${POSTN:-0}" -gt 0 ]; then
            echo "            content: DIR@LANDING differs ($stat) -- never-upstreamed rust-side"
            echo "                     changes, preserved by the sync resolution, whose original"
            echo "                     commits are squashed; carried by the first post-LANDING filtered"
            echo "                     commit's diff"
            UNCLEAN="$UNCLEAN $NAME"
        else
            echo "            content: FAIL -- DIR@LANDING differs ($stat) and NO post-LANDING commit"
            echo "                     touches $DIR: the delta would be silently lost"
            return 1
        fi
    fi
}

# --- derivation for one subtree ---------------------------------------------------------
derive_one() { # $1 = DIR, $2 = standalone ref, $3 = name; prints report, sets DERIVED_FILTER
    local DIR=$1 STANDALONE=$2 NAME=$3
    DERIVED_FILTER=

    local MASTER STAND
    MASTER=$(resolve_at "$RUST_REF")
    STAND=$(resolve_at "$STANDALONE")
    [ -n "$MASTER" ] && [ -n "$STAND" ] || { echo "error: could not resolve refs at '$AT'" >&2; return 2; }

    # TIP: last standalone commit merged into rust = merge base
    local BASES
    BASES=$(g merge-base --all "$MASTER" "$STAND" 2>/dev/null || true)
    if [ -z "$BASES" ]; then
        echo "error: $DIR: no shared history between rust and the standalone repo." >&2
        echo "       This subtree was not synced with git-subtree merges; the plain filter" >&2
        echo "       ':/$DIR' (no :rev entry) is the right starting point." >&2
        return 3
    fi
    if [ "$(echo "$BASES" | wc -l)" -ne 1 ]; then
        echo "error: $DIR: multiple merge bases -- ambiguous TIP, refusing to guess:" >&2
        echo "$BASES" >&2
        return 4
    fi
    local TIP=$BASES

    # Direction guard: the merge base must be a STANDALONE-side commit (last
    # standalone->rust sync). If the standalone repo imports real rust history (e.g.
    # cranelift's "Sync from rust" merges) and that import postdates the last push-back,
    # the merge base is a RUST commit and the derivation would be nonsense.
    # (grep -q on a pipe would SIGPIPE rev-list and, under pipefail, read as a miss)
    local SPINE_TMP
    SPINE_TMP=$(mktemp)
    g rev-list --first-parent "$STAND" > "$SPINE_TMP"
    if grep -qFx "$TIP" "$SPINE_TMP"; then
        : # TIP is on the standalone branch spine -- the normal case
    elif g rev-list --first-parent "$MASTER" > "$SPINE_TMP" && grep -qFx "$TIP" "$SPINE_TMP"; then
        rm -f "$SPINE_TMP"
        echo "error: $DIR: merge base $TIP is a rust-side commit -- the standalone repo has" >&2
        echo "       imported rust history more recently than it was last merged back into" >&2
        echo "       rust. Pick an earlier --at, or wait for the next standalone->rust sync." >&2
        return 4
    else
        echo "warning: merge base $TIP is on neither branch spine; inspect it manually" >&2
    fi
    rm -f "$SPINE_TMP"

    # LANDING: the first-parent commit of MASTER that landed TIP (git-when-merged algorithm).
    # rev-list emits newest-first, so the last ancestry-path line also in the
    # first-parent set is the oldest such commit = the landing merge.
    # (temp files, not <(...): process substitution is a syntax error when bash runs
    # in POSIX mode ("sh script"), and it fails silently inside the $() here)
    local LANDING FP_TMP AP_TMP
    FP_TMP=$(mktemp) AP_TMP=$(mktemp)
    g rev-list --first-parent "$TIP..$MASTER" > "$FP_TMP"
    g rev-list --ancestry-path "$TIP..$MASTER" > "$AP_TMP"
    LANDING=$(awk 'NR==FNR {fp[$0]=1; next} $0 in fp {last=$0} END {print last}' "$FP_TMP" "$AP_TMP")
    rm -f "$FP_TMP" "$AP_TMP"
    if [ -z "$LANDING" ]; then
        echo "error: $DIR: could not locate the landing merge of TIP on the first-parent chain." >&2
        return 4
    fi

    # Inspection refs (written even if the sanity checks below fail -- that is
    # exactly when you want to look at these commits).
    g update-ref "refs/heads/subtree-migration/$NAME/TIP" "$TIP"
    g update-ref "refs/heads/subtree-migration/$NAME/LANDING" "$LANDING"

    # The sync merge: the most ancestral merge on the ancestry path TIP..LANDING. (Not
    # necessarily the merge with TIP as a direct parent: with side commits on the
    # sync branch, the merge lands the last side commit instead of TIP.)
    local SYNC
    SYNC=$(g rev-list --topo-order --ancestry-path --merges "$TIP..$LANDING" | tail -1)

    # Whitelist entries: commits in the squash range whose changes are not part of TIP.
    #  - TIP..SYNC: standalone-layout side commits committed on the sync branch before
    #    the merge -> ==sha:prefix=DIR, reproduced SHA-identically.
    #  - SYNC..LANDING (ancestry path, needs trees): rust-layout fixups on the sync PR branch
    #    touching DIR -> ==sha:/, minted with message/author/diff preserved.
    # Merges and concurrent master-side landings are left to fold into the first
    # post-LANDING filtered commit; the filtered(LANDING) check verifies nothing is lost.
    local WL_ENTRIES= WL_A= WL_C= WL_STAND=0 WL_RUST=0 c
    if [ -n "$SYNC" ]; then
        WL_A=$(g rev-list --topo-order --reverse --ancestry-path "$TIP..$SYNC" \
               | grep -vFx "$SYNC" || true)
        for c in $WL_A; do
            WL_ENTRIES="$WL_ENTRIES,==$c:prefix=$DIR"; WL_STAND=$((WL_STAND+1))
        done
        if [ -z "$(fetch_filter_arg)" ]; then
            WL_C=$(g rev-list --reverse --no-merges --ancestry-path "$SYNC..$LANDING" -- "$DIR")
            for c in $WL_C; do
                WL_ENTRIES="$WL_ENTRIES,==$c:/"; WL_RUST=$((WL_RUST+1))
            done
        else
            echo "warning: $DIR: partial clone: cannot derive rust-side whitelist entries (needs trees)" >&2
        fi
    fi

    # --- sanity checks
    local fail=0
    g merge-base --is-ancestor "$TIP" "$LANDING"      || { echo "FAIL: TIP not ancestor of LANDING" >&2; fail=1; }
    g merge-base --is-ancestor "$LANDING" "$MASTER"   || { echo "FAIL: LANDING not ancestor of rust ref" >&2; fail=1; }
    local UNSYNCED PATHCHECK POSTN=
    UNSYNCED=$(g rev-list --count "$TIP..$STAND")

    # Commits after LANDING touching DIR become the only new filtered commits beyond the
    # whitelist; with none, the filtered history equals the subtree lineage exactly.
    # Also the fold-forward target for any remaining tree-level delta at LANDING. Needs
    # trees, which a partial clone lazily fetches -- skip there.
    if [ -n "$(fetch_filter_arg)" ]; then
        PATHCHECK="SKIPPED (partial clone; inspect in a full clone: git log $LANDING..$MASTER -- $DIR)"
    else
        POSTN=$(g rev-list --count "$LANDING..$MASTER" -- "$DIR")
        PATHCHECK="$POSTN commits touch $DIR after LANDING (these become the only new filtered commits)"
    fi

    # --- report
    show() { g log -1 --date=short --format="    $1 %H%n        %ad %an: %s" "$2"; }
    echo "subtree:    $DIR"
    echo "rust ref:   $RUST_REF @ ${AT:-now}"
    echo "  standalone side:"
    show "head    " "$STAND"
    echo "  shared (in both histories):"
    show "TIP     " "$TIP"
    echo "  rust side:"
    show "head    " "$MASTER"
    [ -n "$SYNC" ] && show "sync    " "$SYNC"
    show "LANDING " "$LANDING"
    echo "checks:     unsynced standalone commits ahead of TIP: $UNSYNCED"
    [ -n "$SYNC" ] || echo "            (no sync merge located -- no whitelist derived; inspect manually)"
    echo "            post-LANDING path check: $PATHCHECK"
    if [ "$WL_STAND" -gt 0 ] || [ "$WL_RUST" -gt 0 ]; then
        echo "whitelist:  $WL_STAND standalone side commit(s) (==sha:prefix), $WL_RUST rust-side fixup(s) (==sha:/):"
        for c in $WL_A $WL_C; do
            g log -1 --date=short --format="              ==%h %ad %an: %s" "$c"
        done
    fi
    echo "refs:       refs/heads/subtree-migration/$NAME/{TIP,LANDING} updated"
    [ "$fail" -eq 0 ] || return 1
    DERIVED_FILTER=":~(history=\"$HISTORY_FLAGS\")[:rev(<=$TIP:prefix=$DIR$WL_ENTRIES,<=$LANDING:SQUASH)]:/$DIR"
    apply_filter "$NAME" "$DERIVED_FILTER" "$MASTER" "$DIR" "$TIP" "$LANDING" "$POSTN" || return 1
    return 0
}

print_unclean_note() {
    [ -n "$UNCLEAN" ] || return 0
    local n
    echo
    echo "NOTE: never-upstreamed rust-side changes detected in:"
    for n in $UNCLEAN; do echo "  - $n"; done
    echo "For these the migrated history starts \"unclean\": the rust-side delta is carried"
    echo "by the diff of the first post-LANDING filtered commit instead of by proper commits."
    echo "Recommended: run one more regular git-subtree sync for these first (upstreaming"
    echo "the rust-side changes), then re-derive. Right after a sync the delta is empty"
    echo "and the filtered history starts exactly at the subtree tip."
}

# --- main -------------------------------------------------------------------------------
if [ -n "$DIR" ] || [ -n "$STANDALONE" ]; then
    [ -n "$DIR" ] && [ -n "$STANDALONE" ] || usage
    [ -n "$RUST_REF" ] || RUST_REF=$(detect_rust_ref)
    derive_one "$DIR" "$STANDALONE" "$(basename "$DIR")" || exit $?
    echo
    echo "filter:"
    echo "$DERIVED_FILTER"
    print_unclean_note
    exit 0
fi

setup
[ -n "$RUST_REF" ] || RUST_REF=$(detect_rust_ref)

RESULTS=
FAILED=0
while read -r name dir url; do
    echo "================================================================"
    if derive_one "$dir" "refs/subtrees/$name" "$name"; then
        RESULTS="$RESULTS$name $DERIVED_FILTER
"
    else
        echo "($name failed, see above)" >&2
        FAILED=1
    fi
    echo
done <<< "$SUBTREES"

echo "================================================================"
echo "filters:"
printf '%s' "$RESULTS" | while read -r name filter; do
    printf '\n%s:\n%s\n' "$name" "$filter"
done
print_unclean_note
exit $FAILED
