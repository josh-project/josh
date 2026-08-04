#!/bin/sh
# Build the josh-starlark-guest interpreter blob for wasm32-unknown-unknown
# and print its absolute path. The blob is multi-MiB and NOT committed; test
# fixtures copy it from the path this script prints.
set -e

cd "$(dirname "$0")/../josh-guest"

# getrandom (transitive via the starlark crate) needs the custom backend on
# wasm32; the deterministic stub lives in josh-starlark-guest. The cfg is also
# set by josh-guest/.cargo/config.toml, but RUSTFLAGS makes this script work
# regardless of cargo config discovery.
RUSTFLAGS='--cfg getrandom_backend="custom"' \
    cargo build --release --target wasm32-unknown-unknown \
    -p josh-starlark-guest 1>&2

echo "$(pwd)/target/wasm32-unknown-unknown/release/josh_starlark_guest.wasm"
