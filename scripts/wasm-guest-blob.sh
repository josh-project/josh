#!/bin/sh
# Print the absolute path of a built guest wasm blob, building it on demand.
#
# Usage: wasm-guest-blob.sh starlark|example
#
# The build is skipped when the artifact already exists; set JOSH_GUEST_REBUILD
# to force a rebuild. For containerized runs without a cargo toolchain,
# JOSH_STARLARK_GUEST_WASM overrides the starlark blob location with a
# pre-built file. run-tests.sh prepends scripts/ to PATH, so scrut test
# fixtures call this helper by name.
set -e

kind="${1:?usage: wasm-guest-blob.sh starlark|example}"

root="$(cd "$(dirname "$0")/.." && pwd)"
release_dir="${root}/josh-guest/target/wasm32-unknown-unknown/release"

case "${kind}" in
starlark)
    if [ -n "${JOSH_STARLARK_GUEST_WASM}" ]; then
        echo "${JOSH_STARLARK_GUEST_WASM}"
        exit 0
    fi
    blob="${release_dir}/josh_starlark_guest.wasm"
    if [ ! -f "${blob}" ] || [ -n "${JOSH_GUEST_REBUILD}" ]; then
        sh "${root}/scripts/build-starlark-guest.sh" 1>&2
    fi
    ;;
example)
    blob="${release_dir}/subdirs_prefixed.wasm"
    if [ ! -f "${blob}" ] || [ -n "${JOSH_GUEST_REBUILD}" ]; then
        (
            cd "${root}/josh-guest" &&
                cargo build --release --target wasm32-unknown-unknown \
                    -p subdirs-prefixed 1>&2
        )
    fi
    ;;
*)
    echo "unknown guest blob: ${kind}" >&2
    exit 1
    ;;
esac

echo "${blob}"
