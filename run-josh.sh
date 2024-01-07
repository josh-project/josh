#!/bin/bash
cd /josh/
RUST_BACKTRACE=1 ./target/debug/josh-proxy --gc --local=~/data/git --remote="${JOSH_REMOTE}" ${JOSH_EXTRA_OPTS}
