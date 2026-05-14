#!/usr/bin/env bash
set -euxo pipefail

PODMAN_VERSION="v5.8.2"
PODMAN_KEY_FPR="0CCF102C4F95D89E583FF1D4F8B5AF50344BB503"

PODMAN_DIR="${GITHUB_WORKSPACE}/.github/podman"
TARBALL="podman-linux-amd64.tar.gz"
URL="https://github.com/mgoltzsche/podman-static/releases/download/${PODMAN_VERSION}/${TARBALL}"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

curl -fsSL -o "${WORK}/${TARBALL}" "${URL}"

GNUPGHOME="$(mktemp -d)"
chmod 700 "${GNUPGHOME}"
export GNUPGHOME
gpg --batch --import "${PODMAN_DIR}/maintainer-pubkey.asc"
gpg --batch --list-keys "${PODMAN_KEY_FPR}" >/dev/null
gpg --batch --verify "${PODMAN_DIR}/${TARBALL}.asc" "${WORK}/${TARBALL}"

PREFIX="/opt/podman-${PODMAN_VERSION#v}"
sudo mkdir -p /opt
sudo tar -xzf "${WORK}/${TARBALL}" -C /opt
sudo mv /opt/podman-linux-amd64 "${PREFIX}"
sudo ln -sf "${PREFIX}/usr/local/bin/podman" /usr/local/bin/podman
podman --version
