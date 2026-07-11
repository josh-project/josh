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
for bin in podman crun runc pasta fuse-overlayfs fusermount3; do
    sudo ln -sf "${PREFIX}/usr/local/bin/${bin}" "/usr/local/bin/${bin}"
done

sudo mkdir -p /etc/containers
sudo install -m 0644 "${PODMAN_DIR}/containers.conf" /etc/containers/containers.conf

if [ -f /etc/apparmor.d/podman ]; then
    sudo install -m 0644 "${PODMAN_DIR}/apparmor-podman" /etc/apparmor.d/podman
    sudo apparmor_parser -r /etc/apparmor.d/podman
fi

podman --version
