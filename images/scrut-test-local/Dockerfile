# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE
FROM ${BASE_IMAGE}

RUN <<EOF
set -eux
mkdir -p /opt/cache /opt/target /opt/build
chmod 777 /opt/cache /opt/target /opt/build
EOF

VOLUME /opt/cache

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
