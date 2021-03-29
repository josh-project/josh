#!/bin/bash
cd /josh/
export GIT_AUTHOR_NAME=Josh
export GIT_AUTHOR_EMAIL=josh@example.com
export GIT_COMMITTER_NAME=Josh
export GIT_COMMITTER_EMAIL=josh@example.com
RUST_BACKTRACE=1 josh-proxy --gc --local=/data/git/ --remote="${JOSH_REMOTE}" ${JOSH_EXTRA_OPTS}
