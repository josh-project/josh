#!/usr/bin/env sh

set -u

export TESTTMP=${PWD}

killall josh-proxy >/dev/null 2>&1 || true
killall hyper-cgi-test-server >/dev/null 2>&1 || true
killall josh-ssh-dev-server >/dev/null 2>&1 || true

export GIT_CONFIG_NOSYSTEM=1
git init -q --bare "${TESTTMP}/remote/real_repo.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/real_repo.git/config" http.receivepack true
git init -q --bare "${TESTTMP}/remote/blocked_repo.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/blocked_repo.git/config" http.receivepack true
git init -q --bare "${TESTTMP}/remote/real/repo2.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/real/repo2.git/config" http.receivepack true
git init -q --bare "${TESTTMP}/remote/meta_repo.git/" 1> /dev/null
git config -f "${TESTTMP}/remote/meta_repo.git/config" http.receivepack true

export RUST_LOG=trace
export JOSH_SERVICE_NAME="josh-proxy-test"
export JOSH_REPO_BLOCK="/blocked_repo.git"

GIT_DIR="${TESTTMP}/remote/" GIT_PROJECT_ROOT="${TESTTMP}/remote/" GIT_HTTP_EXPORT_ALL=1 hyper-cgi-test-server \
    --port=8001 \
    --dir="${TESTTMP}/remote/" \
    --cmd=git \
    --args=http-backend \
    > "${TESTTMP}/hyper-cgi-test-server.out" 2>&1 &
echo $! > "${TESTTMP}/server_pid"

# Copy static UI resources
cp -R "${TESTDIR}/../../static/" /josh/

if [ -n "${CARGO_TARGET_DIR+x}" ]; then
    export TARGET_DIR=${CARGO_TARGET_DIR}
else
    export TARGET_DIR=${TESTDIR}/../../target
fi

if [ -n "${JOSH_TEST_SSH+x}" ]; then
    SSH_OPTS="--remote=ssh://git@localhost:9002"
else
    SSH_OPTS=""
fi

if [ -z "${EXTRA_OPTS+x}" ]; then
    EXTRA_OPTS=""
fi

# shellcheck disable=SC2086
"${TARGET_DIR}/debug/josh-proxy" \
    --port=8002 \
    --local="${TESTTMP}/remote/scratch/" \
    --remote=http://localhost:8001 \
    ${SSH_OPTS} \
    ${EXTRA_OPTS} \
    > "${TESTTMP}/josh-proxy.out" 2>&1 &
echo $! > "${TESTTMP}"/proxy_pid

SSH_TEST_SERVER="${TARGET_DIR}/josh-ssh-dev-server"

if [ -n "${JOSH_TEST_SSH+x}" ]; then
    if [ -n "${SSH_AUTH_SOCK+x}" ]; then
        unset SSH_AUTH_SOCK
    fi

    # Start SSH agent
    eval "$(ssh-agent)" >/dev/null 2>&1

    # SSH server 1: calls josh-ssh-shell
    JOSH_SSH_SHELL_ENDPOINT_PORT=8002 \
    RUST_LOG=error \
    "${SSH_TEST_SERVER}" \
        -shell="${TARGET_DIR}/debug/josh-ssh-shell" \
        -port=9001 \
        > "${TESTTMP}/ssh-server-1.out" 2>&1 &
    echo $! > "${TESTTMP}"/ssh_server_1_pid

    # SSH server 2: serves as remote for Josh
    "${SSH_TEST_SERVER}" \
        -port=9002 \
        > "${TESTTMP}/ssh-server-2.out" 2>&1 &
    echo $! > "${TESTTMP}"/ssh_server_2_pid

    sleep 1
fi

COUNTER=0
until curl -s http://localhost:8002/
do
    sleep 0.1
    COUNTER=$((COUNTER + 1))
    if [ ${COUNTER} -ge 20 ]; then
        >&2 echo "Starting josh proxy timed out"
        cat "${TESTTMP}/josh-proxy.out" >&2
        exit 1
    fi
done
