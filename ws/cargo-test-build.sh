set -e

cargo test --workspace --offline --locked --no-run --message-format=json \
    > /tmp/cargo.json

sh check-sccache.sh

mkdir -p /out/test-bins

jq -r 'select(.executable != null) | .executable | select(contains("/debug/deps/"))' \
    /tmp/cargo.json \
    | while read -r bin; do
        cp "$bin" /out/test-bins/
    done
