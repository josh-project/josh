#!/bin/bash
DATE=$(date)
mkdir -p "/data/logs/${DATE}/"
RUST_BACKTRACE=1 josh-proxy --local=/tmp/josh-scratch/ --remote=https://gerrit.int.esrlabs.com --trace "/data/logs/${DATE}/" | tee "/data/logs/${DATE}/out.log"
