#!/bin/bash

cd /josh/ || exit 1

# shellcheck disable=SC2086
# intended to pass along the arguments
RUST_BACKTRACE=1 josh-proxy --gc --local=/data/git/ --remote="${JOSH_REMOTE}" ${JOSH_EXTRA_OPTS}
