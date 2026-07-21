set -e

cargo build --workspace --tests --offline --locked
cargo build --workspace --offline --locked

# Workspace members were compiled against stub sources, so their rlibs and
# fingerprints must NOT leak into the downstream build -- otherwise cargo would
# treat the stub rlibs as fresh and link broken empty modules into the real
# build. Strip them; external rlibs and the registry cache remain.
cargo metadata --format-version 1 --no-deps --offline --locked \
    | jq -r '.packages[].name' \
    | while read -r name; do
        cargo clean --offline --locked -p "$name" 2>/dev/null || true
    done

mkdir -p /out/target /out/build
cp -a --reflink=auto "${CARGO_TARGET_DIR}/." /out/target/
cp -a --reflink=auto "${CARGO_BUILD_BUILD_DIR}/." /out/build/

sh check-sccache.sh
