set -e

cp -a --reflink=auto /devtools-deps-build/target/. "${CARGO_TARGET_DIR}/"
cp -a --reflink=auto /devtools-deps-build/build/. "${CARGO_BUILD_BUILD_DIR}/"

cd devtools/git-tree-pretty

cargo fmt -- --check
cargo build --offline --locked
mkdir -p /out/debug
cp ${CARGO_TARGET_DIR}/debug/git-tree-pretty /out/debug/
cargo test --offline --locked

cd ../..
sh check-sccache.sh
