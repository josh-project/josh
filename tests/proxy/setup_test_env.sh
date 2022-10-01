export TESTTMP=${PWD}

killall josh-proxy >/dev/null 2>&1 || true
killall hyper-cgi-test-server >/dev/null 2>&1 || true

git init -q --bare "${TESTTMP}/remote/real_repo.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/real_repo.git/config" http.receivepack true
git init -q --bare "${TESTTMP}/remote/blocked_repo.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/blocked_repo.git/config" http.receivepack true
git init -q --bare "${TESTTMP}/remote/real/repo2.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/real/repo2.git/config" http.receivepack true
git init -q --bare "${TESTTMP}/remote/meta_repo.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/meta_repo.git/config" http.receivepack true
export RUST_LOG=trace

export GIT_CONFIG_NOSYSTEM=1
export JOSH_SERVICE_NAME="josh-proxy-test"
export JOSH_REPO_BLOCK="/blocked_repo.git"

GIT_DIR="${TESTTMP}/remote/" GIT_PROJECT_ROOT="${TESTTMP}/remote/" GIT_HTTP_EXPORT_ALL=1 hyper-cgi-test-server\
    --port=8001\
    --dir="${TESTTMP}/remote/"\
    --cmd=git\
    --args=http-backend\
    > "${TESTTMP}/hyper-cgi-test-server.out" 2>&1 &
echo $! > "${TESTTMP}/server_pid"

cp -R "${TESTDIR}/../../static/" static

if [ -z "${CARGO_TARGET_DIR}" ]; then
    export TARGET_DIR=${TESTDIR}/../../target
else
    export TARGET_DIR=${CARGO_TARGET_DIR}
fi

# shellcheck disable=SC2086
"${TARGET_DIR}/debug/josh-proxy" \
    --port=8002\
    --local="${TESTTMP}/remote/scratch/"\
    --remote=http://localhost:8001\
    ${EXTRA_OPTS}\
    > "${TESTTMP}/josh-proxy.out" 2>&1 &
echo $! > "${TESTTMP}"/proxy_pid

COUNTER=0
until curl -s http://localhost:8002/
do
    sleep 0.1
    COUNTER=$((COUNTER + 1))
    if [ $COUNTER -ge 20 ];
    then
        >&2 echo "Starting josh proxy timed out"
        cat ${TESTTMP}/josh-proxy.out >&2
        exit 1
    fi
done
