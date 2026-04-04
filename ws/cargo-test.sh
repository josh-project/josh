set -e

export RUSTFLAGS="-D warnings"
cargo test --workspace --offline --locked
