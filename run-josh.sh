#!/bin/bash
DATE=$(date)
mkdir -p "/data/logs/${DATE}/"
RUST_BACKTRACE=1 josh-proxy --gc --local=/data/git/ --remote="${JOSH_REMOTE}" --trace "/data/logs/${DATE}/" | tee "/data/logs/${DATE}/out.log"
