#!/bin/bash
${TESTDIR}/../../target/debug/josh-proxy -m --local=${TESTTMP}/remote/scratch/ > /dev/null 2>&1
cd ${TESTTMP}; tree remote/scratch/refs
#cat ${TESTTMP}/josh-proxy.out
killall -2 josh-proxy

