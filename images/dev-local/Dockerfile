# syntax=docker/dockerfile:1.15.1@sha256:9857836c9ee4268391bb5b09f9f157f3c91bb15821bb77969642813b0d00518d

ARG BASE_IMAGE
FROM ws_image_${BASE_IMAGE}

RUN <<EOF
set -eux
mkdir -p /opt/cache /opt/target /josh/static
chmod 777 /opt/cache /opt/target /josh/static
EOF

VOLUME /opt/cache

ENV CARGO_TARGET_DIR=/opt/target/cargo-target
ENV CARGO_BUILD_BUILD_DIR=/opt/cache/cargo-build
ENV CARGO_HOME=/opt/cache/cargo-cache
ENV GOCACHE=/opt/cache/go-cache
ENV GOPATH=/opt/target/go-path
ENV GOFLAGS=-buildvcs=false
RUN npm config set cache /opt/cache/npm-cache --global

ARG USER_GID
ARG USER_UID

RUN <<EOF
set -eux

if [ ! $(getent group ${USER_GID}) ] ; then
    addgroup -g ${USER_GID} dev
fi

adduser \
    -u ${USER_UID} \
    -G $(getent group ${USER_GID} | cut -d: -f1) \
    -D \
    -H \
    -g '' \
    dev
EOF
