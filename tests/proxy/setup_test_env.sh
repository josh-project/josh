export TESTTMP=${PWD}
killall josh-proxy >/dev/null 2>&1
killall josh-test-server >/dev/null 2>&1

git init --bare ${TESTTMP}/remote/real_repo.git/ 1> /dev/null
git config -f ${TESTTMP}/remote/real_repo.git/config http.receivepack true
git init --bare ${TESTTMP}/remote/real/repo2.git/ 1> /dev/null
git config -f ${TESTTMP}/remote/real/repo2.git/config http.receivepack true
export RUST_LOG=debug

export TESTPASS=$(openssl rand -hex 5)
export TESTUSER=$(openssl rand -hex 5)

PATH=${TESTDIR}/../../target/debug/:${PATH}

${TESTDIR}/../../target/debug/josh-test-server\
    --port=8001\
    --local=${TESTTMP}/remote/\
    --username=${TESTUSER}\
    --password=${TESTPASS}\
    > ${TESTTMP}/josh-test-server.out 2>&1 &
echo $! > ${TESTTMP}/server_pid

${TESTDIR}/../../target/debug/josh-proxy2\
    --port=8002\
    --local=${TESTTMP}/remote/scratch/\
    --remote=http://localhost:8001\
    > ${TESTTMP}/josh-proxy.out 2>&1 &
echo $! > ${TESTTMP}/proxy_pid

sleep 1
