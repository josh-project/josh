#!/bin/bash
curl -s http://localhost:8002/filters/refresh
killall -2 josh-proxy
cd ${TESTTMP}/remote/scratch
#${TESTDIR}/../../target/debug/josh-filter -vs
tree refs
#cat ${TESTTMP}/josh-proxy.out

