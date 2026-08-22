set -e

cp -a --reflink=auto /devtools-deps-build/target/. "${CARGO_TARGET_DIR}/"
cp -a --reflink=auto /devtools-deps-build/build/. "${CARGO_BUILD_BUILD_DIR}/"

cd devtools

cargo fmt -- --check
cargo build --offline --locked
cargo test --offline --locked

cd ..
sh check-sccache.sh
