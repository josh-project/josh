set -e

cp -a --reflink=auto /josh-gui-deps-build/target/. "${CARGO_TARGET_DIR}/"
cp -a --reflink=auto /josh-gui-deps-build/build/. "${CARGO_BUILD_BUILD_DIR}/"

cd josh-gui

cargo fmt -- --check
cargo build --offline --locked

cd ..
sh check-sccache.sh
