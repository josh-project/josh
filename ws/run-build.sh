set -e

export RUSTFLAGS="-D warnings"
cargo fetch --locked || true
cargo build --workspace $CARGO_BUILD_FEATURES --offline --locked
( cd josh-ssh-dev-server; go build -o "${CARGO_TARGET_DIR}/josh-ssh-dev-server" )

mkdir /out/debug
cp ${CARGO_TARGET_DIR}/debug/josh /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-proxy /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-cq /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-filter /out/debug/
cp ${CARGO_TARGET_DIR}/debug/josh-ssh-shell /out/debug/
cp ${CARGO_TARGET_DIR}/debug/axum-cgi-server /out/debug/
cp ${CARGO_TARGET_DIR}/josh-ssh-dev-server /out/debug/
cp -R ./static /out/static
