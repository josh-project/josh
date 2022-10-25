#!/usr/bin/env bash

# this script can be used to run tests in the container used for deployement.
# usage:
#  Run all tests: ./tester.sh
#  Run specific test: ./tester.sh path/to/test.t

set -e
shopt -s extglob
shopt -s inherit_errexit

if (( $# == 0 )); then
    tests="tests/{filter{**/,},proxy}/*.t"
else
    tests="$*"
fi

echo "running: $tests"

docker buildx build --target=dev-local -t josh-dev-local .
docker run -it --rm\
    --workdir "$(pwd)"\
    --volume "$(pwd)":"$(pwd)"\
    --volume cache:/opt/cache\
    --user "$(id -u)":"$(id -g)"\
    josh-dev-local\
    bash -c "cargo build --workspace --exclude josh-ui --features hyper_cgi/test-server && sh run-tests.sh $tests"

