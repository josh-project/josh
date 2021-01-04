#!/bin/bash
killall -2 josh-proxy
${TESTDIR}/../../target/debug/josh-proxy -m --local=${TESTTMP}/remote/scratch/ > /dev/null 2>&1
cd ${TESTTMP}/remote/scratch
#${TESTDIR}/../../target/debug/josh-filter -vs
tree refs
#cat ${TESTTMP}/josh-proxy.out

