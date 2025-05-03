#!/usr/bin/env bash

# this script can be used to run tests in the container used for deployment.
# usage:
#  Run all tests: ./tester.sh
#  Run specific test: ./tester.sh path/to/test.t

set -e
shopt -s extglob
shopt -s inherit_errexit

if (( $# >= 1 )) && [[ "${1}" == "--no-build-container" ]]; then
    NO_BUILD_CONTAINER=1
    shift
else
    NO_BUILD_CONTAINER=0
fi

if (( $# == 0 )); then
    TESTS="tests/{filter{**/,},proxy}/*.t"
else
    TESTS="$*"
fi

echo "running: ${TESTS}"

if (( ! NO_BUILD_CONTAINER )); then
    docker buildx build \
        --target=dev-local \
        --tag=josh-dev-local \
        --build-arg USER_UID="$(id -u)" \
        --build-arg USER_GID="$(id -g)" \
        .
fi

mapfile -d '' TEST_SCRIPT << EOF
set -e

if [[ ! -v CARGO_TARGET_DIR ]]; then
    echo "CARGO_TARGET_DIR not set"
    exit 1
fi

export RUSTFLAGS="-D warnings"
cargo build --workspace --exclude josh-ui --features hyper_cgi/test-server
( cd josh-ssh-dev-server ; go build -o "\${CARGO_TARGET_DIR}/josh-ssh-dev-server" )
sh run-tests.sh ${TESTS}
EOF

docker run -it --rm \
    --workdir "$(pwd)" \
    --volume "$(pwd)":"$(pwd)" \
    --volume cache:/opt/cache \
    --user "$(id -u)":"$(id -g)" \
    josh-dev-local \
    bash -c "${TEST_SCRIPT}"
