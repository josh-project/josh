set -e
cargo build --all
export PATH=$(pwd)/target/debug/:${PATH}
export PATH=$(pwd)/scripts/:${PATH}
python3 -m cram "$@"
