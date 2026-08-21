set -e

cp -a --reflink=auto /cargo-deps-build/target/. "${CARGO_TARGET_DIR}/"
cp -a --reflink=auto /cargo-deps-build/build/. "${CARGO_BUILD_BUILD_DIR}/"

# On failure the diagnostics are in the json, so render them before giving up: a build
# that fails silently is unactionable from the run log.
cargo test --workspace --offline --locked --no-run --message-format=json \
    > /tmp/cargo.json \
    || { jq -r 'select(.message.rendered != null) | .message.rendered' /tmp/cargo.json; exit 1; }

sh check-sccache.sh

mkdir -p /out/test-bins

jq -r 'select(.executable != null) | .executable | select(contains("/debug/deps/"))' \
    /tmp/cargo.json \
    | while read -r bin; do
        cp "$bin" /out/test-bins/
    done
