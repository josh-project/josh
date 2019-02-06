#!/bin/bash
mkdir -p /data/logs
RUST_BACKTRACE=1 josh-proxy --local=/tmp/josh-scratch/ --remote=https://gerrit.int.esrlabs.com | tee "/data/logs/$(date).log"
