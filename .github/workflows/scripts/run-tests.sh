#!/usr/bin/env bash

set -e -x

# Formatting
cargo fmt -- --check

# Unit tests
cargo test --workspace --all --exclude josh-link --exclude josh-cq --exclude josh-starlark

# Integration tests
cargo build --workspace --all-targets --exclude josh-link --exclude josh-cq --exclude josh-starlark
( cd josh-ssh-dev-server ; go build -o "${CARGO_TARGET_DIR}/josh-ssh-dev-server" )
sh run-tests.sh --verbose tests/filter/**.t
sh run-tests.sh --verbose tests/proxy/**.t
sh run-tests.sh --verbose tests/cli/**.t
