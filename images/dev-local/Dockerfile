# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-dev
FROM ${BASE_IMAGE}

RUN <<EOF
set -eux
mkdir -p /opt/cache /opt/target
chmod 777 /opt/cache /opt/target
EOF

VOLUME /opt/cache

ENV CARGO_TARGET_DIR=/opt/cache/cargo-target
ENV CARGO_HOME=/opt/cache/cargo-cache
ENV GOCACHE=/opt/cache/go-cache
ENV GOPATH=/opt/cache/go-path
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
