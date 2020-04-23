#!/bin/bash
${TESTDIR}/../../target/debug/josh-proxy --m --local=${TESTTMP}/remote/scratch/ &> /dev/null
cd ${TESTTMP}; tree remote/scratch/refs
#cat ${TESTTMP}/josh-proxy.out
