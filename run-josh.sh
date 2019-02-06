#!/bin/bash
RUST_BACKTRACE=1 josh-proxy --local=/tmp/josh-scratch/ --remote=https://gerrit.int.esrlabs.com | tee "/data/logs/$(date).log"

