set -e

export RUSTFLAGS="-D warnings"
cargo fetch --locked || true
cargo build --workspace --features incubating --offline --locked
( cd josh-ssh-dev-server; go build -o "${CARGO_TARGET_DIR}/josh-ssh-dev-server" )

mkdir /out/debug
cp /opt/cache/cargo-target/debug/josh /out/debug/
cp /opt/cache/cargo-target/debug/josh-proxy /out/debug/
cp /opt/cache/cargo-target/debug/josh-cq /out/debug/
cp /opt/cache/cargo-target/debug/josh-filter /out/debug/
cp /opt/cache/cargo-target/debug/josh-ssh-shell /out/debug/
cp /opt/cache/cargo-target/debug/axum-cgi-server /out/debug/
cp /opt/cache/cargo-target/josh-ssh-dev-server /out/debug/
