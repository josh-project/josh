set -e

export RUSTFLAGS="-D warnings -Ctarget-feature=-crt-static"
cargo build --workspace $CARGO_BUILD_FEATURES --offline --locked

mkdir /out/debug
cp ${CARGO_TARGET_DIR}/debug/josh /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-proxy /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-cq /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-filter /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-ssh-shell /out/debug/
cp ${CARGO_TARGET_DIR}/debug/axum-cgi-server /out/debug/
