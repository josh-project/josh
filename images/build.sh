#!/bin/bash
set -e

cd "$(dirname "$0")/.."

ARCH=$(uname -m)
case "$ARCH" in
    x86_64)        ARCH=amd64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
esac

podman build -f images/rust-base/Dockerfile --build-arg ARCH=$ARCH -t josh-rust-base .
podman build -f images/dev/Dockerfile --build-arg ARCH=$ARCH -t josh-dev .
podman build -f images/dev-ci/Dockerfile -t josh-dev-ci .
podman build -f images/build/Dockerfile --build-arg ARCH=$ARCH --build-context git=.git -t josh-build .
podman build -f images/run/Dockerfile --build-arg ARCH=$ARCH -t josh .
