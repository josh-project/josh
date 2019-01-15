killall grib &> /dev/null
killall git-http-server &> /dev/null
git init --bare ${CRAMTMP}/remote/real_repo.git/ &> /dev/null
git config -f ${CRAMTMP}/remote/real_repo.git/config http.receivepack true
export RUST_LOG=debug

${TESTDIR}/../target/debug/git-http-server\
    --port=8001\
    --local=${CRAMTMP}/remote/\
    &> ${CRAMTMP}/git-http-server.out&
echo $! > ${CRAMTMP}/server_pid

${TESTDIR}/../target/debug/grib\
    --port=8002\
    --local=${CRAMTMP}/remote/scratch/\
    --remote=http://localhost:8001\
    &> ${CRAMTMP}/grib.out&
echo $! > ${CRAMTMP}/proxy_pid
