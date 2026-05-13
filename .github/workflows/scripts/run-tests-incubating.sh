#!/usr/bin/env bash

set -e -x

export JOSH_EXPERIMENTAL_FEATURES=1

# Formatting
cargo fmt -- --check

# Unit tests
cargo test --workspace --all

# Integration tests
cargo build --workspace --all-targets
( cd josh-ssh-dev-server ; go build -o "${CARGO_TARGET_DIR}/josh-ssh-dev-server" )
sh run-tests.sh --verbose tests/experimental/**.t
sh run-tests.sh --verbose tests/filter/**.t
sh run-tests.sh --verbose tests/proxy/**.t
sh run-tests.sh --verbose tests/cli/**.t
sh run-tests.sh --verbose tests/cq/**.t
