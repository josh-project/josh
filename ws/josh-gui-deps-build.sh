set -e

cd josh-gui

cargo build --offline --locked

# Cargo considers stub-built workspace artifacts fresh, so remove them before the real build.
cargo metadata --format-version 1 --offline --locked \
    | jq -r '.packages[]
        | select(.manifest_path | startswith("/worktree/"))
        | .name' \
    | while read -r name; do
        cargo clean --offline --locked -p "$name" 2>/dev/null || true
    done

cd ..

mkdir -p /out/target /out/build
cp -a --reflink=auto "${CARGO_TARGET_DIR}/." /out/target/
cp -a --reflink=auto "${CARGO_BUILD_BUILD_DIR}/." /out/build/

sh check-sccache.sh
