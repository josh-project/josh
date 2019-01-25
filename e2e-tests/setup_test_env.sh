export TESTTMP=${PWD}
killall grib &> /dev/null
killall git-http-server &> /dev/null
git init --bare ${TESTTMP}/remote/real_repo.git/ &> /dev/null
git config -f ${TESTTMP}/remote/real_repo.git/config http.receivepack true
export RUST_LOG=debug

${TESTDIR}/../target/debug/git-http-server\
    --port=8001\
    --local=${TESTTMP}/remote/\
    &> ${TESTTMP}/git-http-server.out&
echo $! > ${TESTTMP}/server_pid

${TESTDIR}/../target/debug/grib\
    --port=8002\
    --local=${TESTTMP}/remote/scratch/\
    --remote=http://localhost:8001\
    &> ${TESTTMP}/grib.out&
echo $! > ${TESTTMP}/proxy_pid
