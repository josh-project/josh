set -e

cd devtools

cargo build --offline --locked
cargo test --offline --locked --no-run

# Workspace-local crates were compiled against stub sources; their rlibs and
# fingerprints must NOT leak into the downstream build, or cargo would treat
# the stubs as fresh and link broken empty modules into the real build. Strip
# every package whose manifest lives under the worktree (i.e. all path-local
# crates reachable from devtools); external rlibs and the registry remain.
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
