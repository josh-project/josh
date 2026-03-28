#!/usr/bin/env bash

set -e -x
shopt -s extglob

# Formatting
cargo fmt -- --check

# Guard against cargo feature unification silently enabling "incubating" on josh-core.
# When building the whole workspace, any crate that depends on josh-core with
# features=["incubating"] causes ALL crates to see josh-core with that feature on.
# This check ensures the non-incubating build genuinely compiles without it.
declare incubating_enabled
declare cargo_tree_output

cargo_tree_output=$(cargo tree -p josh-core -e features --workspace \
  --exclude josh-link --exclude josh-cq --exclude josh-starlark --prefix none)
incubating_enabled=$(echo "$cargo_tree_output" | grep 'josh-core feature "incubating"' | wc -l)

if [ "$incubating_enabled" -ne 0 ]; then
  echo "ERROR: incubating feature is leaking into the non-incubating workspace build"
  exit 1
fi

# Unit tests
cargo test --workspace --all --exclude josh-link --exclude josh-cq --exclude josh-starlark

# Integration tests
cargo build --workspace --all-targets --exclude josh-link --exclude josh-cq --exclude josh-starlark
( cd josh-ssh-dev-server ; go build -o "${CARGO_TARGET_DIR}/josh-ssh-dev-server" )
sh run-tests.sh --verbose tests/filter/!(incubating_*).t
sh run-tests.sh --verbose tests/proxy/**.t
sh run-tests.sh --verbose tests/cli/**.t
