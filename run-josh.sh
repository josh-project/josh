#!/bin/bash
[ -n "${JOSH_ACL}" ] && acl_arg="--acl ${JOSH_ACL}"
cd /josh/
RUST_BACKTRACE=1 josh-proxy --gc --local=/data/git/ --remote="${JOSH_REMOTE}" ${acl_arg} ${JOSH_EXTRA_OPTS}
