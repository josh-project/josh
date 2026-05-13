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
    TESTS="tests/{filter{**/,},proxy,cli,cq,experimental}/*.t"
else
    TESTS="$*"
fi

echo "running: ${TESTS}"

if (( ! NO_BUILD_CONTAINER )); then
    docker buildx bake \
        --set "dev-local.args.USER_UID=$(id -u)" \
        --set "dev-local.args.USER_GID=$(id -g)" \
        dev-local
fi

mapfile -d '' TEST_SCRIPT << EOF
set -e

if [[ ! -v CARGO_TARGET_DIR ]]; then
    echo "CARGO_TARGET_DIR not set"
    exit 1
fi

export RUSTFLAGS="-D warnings"
cargo build --workspace
( cd josh-ssh-dev-server ; go build -o "\${CARGO_TARGET_DIR}/josh-ssh-dev-server" )
sh run-tests.sh ${TESTS}
EOF

DOCKER_RUN_FLAGS=(--interactive)
if [ -t 0 ] && [ -t 1 ]; then
    DOCKER_RUN_FLAGS+=(--tty)
fi

docker run --rm \
    "${DOCKER_RUN_FLAGS[@]}" \
    --workdir "$(pwd)" \
    --volume "$(pwd)":"$(pwd)" \
    --volume cache:/opt/cache \
    --user "$(id -u)":"$(id -g)" \
    -e JOSH_EXPERIMENTAL_FEATURES=1 \
    josh-dev-local \
    bash -c "${TEST_SCRIPT}"
