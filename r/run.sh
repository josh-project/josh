set -e

if [[ ! -v CARGO_TARGET_DIR ]]; then
    echo "CARGO_TARGET_DIR not set"
    exit 1
fi

export RUSTFLAGS="-D warnings"
rustc -vV
cargo build --workspace --exclude josh-ui --features hyper_cgi/test-server -v
( cd josh-ssh-dev-server ; go build -o "\${CARGO_TARGET_DIR}/josh-ssh-dev-server" )
sh run-tests.sh ${@:1:99}
