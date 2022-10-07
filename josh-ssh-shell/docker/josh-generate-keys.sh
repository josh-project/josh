#!/usr/bin/env bash

set -e
shopt -s inherit_errexit

KEY_DIR=/mnt/persistent/.ssh
KEY_TYPE=ed25519

function _ensure_owner() {
    TARGET=$1
    OWNER=$2

    if [[ $(stat -c "%G:%U" "${TARGET}") != "${OWNER}" ]]; then
        chown "${OWNER}" "${TARGET}"
    fi
}

function _ensure_mode() {
    TARGET=$1
    MODE=$2

    if [[ $(stat -c "%a" "${TARGET}") != "${MODE}" ]]; then
        chmod "${MODE}" "${TARGET}"
    fi
}

function _ensure_dir() {
    TARGET=$1

    if [[ ! -d "${TARGET}" ]]; then
        mkdir -p "${TARGET}"
    fi
}

function _create_keys() {
    if [[ ! -d /mnt/persistent ]]; then
        2>&1 echo "Persistent volume not mounted"
        exit 1
    fi

    _ensure_dir ${KEY_DIR}
    _ensure_owner ${KEY_DIR} git:git
    _ensure_mode ${KEY_DIR} 700

    if {
        [[ ! -f ${KEY_DIR}/id_${KEY_TYPE} ]] || [[ ! -f ${KEY_DIR}/id_${KEY_TYPE}.pub ]]
    }; then
        2>&1 echo "Generating SSH server key"
        ssh-keygen -t ${KEY_TYPE} -N "" -f ${KEY_DIR}/id_${KEY_TYPE} -C git
    fi

    _ensure_owner ${KEY_DIR}/id_${KEY_TYPE} git:git
    _ensure_mode ${KEY_DIR}/id_${KEY_TYPE} 600

    _ensure_owner ${KEY_DIR}/id_${KEY_TYPE}.pub git:git
    _ensure_mode ${KEY_DIR}/id_${KEY_TYPE}.pub 644
}

_create_keys
