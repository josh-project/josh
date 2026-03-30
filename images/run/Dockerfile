# syntax=docker/dockerfile:1.15.1@sha256:9857836c9ee4268391bb5b09f9f157f3c91bb15821bb77969642813b0d00518d

ARG DEV_IMAGE=josh-dev
ARG BUILD_IMAGE=josh-build
ARG ALPINE_VERSION=3.22.0

FROM ${DEV_IMAGE} AS source-dev
FROM ${BUILD_IMAGE} AS source-build
FROM alpine:${ALPINE_VERSION}

RUN apk add --no-cache \
    zlib \
    openssl \
    libexpat \
    libgit2 \
    libgcc \
    libcurl \
    ca-certificates \
    openssh \
    bash \
    xz \
    shadow \
    gettext

COPY --from=source-dev --link=false /opt/git-install /opt/git-install
ENV PATH=${PATH}:/opt/git-install/bin

COPY --from=source-build --link=false /opt/cargo-target/release/josh-proxy /usr/bin/
COPY --from=source-build --link=false /opt/cargo-target/release/josh-ssh-shell /usr/bin/
COPY --from=source-build --link=false /usr/src/josh/static/ /josh/static/

ARG S6_OVERLAY_VERSION=3.2.1.0
ARG ARCH
RUN <<EOF
set -eux

apk add --no-cache curl

curl -sSL https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-noarch.tar.xz \
    -o /tmp/s6-overlay-noarch.tar.xz
tar -C / -Jxpf /tmp/s6-overlay-noarch.tar.xz
rm /tmp/s6-overlay-noarch.tar.xz

if [ "$ARCH" = amd64 ]; then
    s6_arch=x86_64;
elif [ "$ARCH" = arm64 ]; then
    s6_arch=aarch64;
else
    echo "Unsupported arch";
    exit 1
fi

curl -sSL https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-${s6_arch}.tar.xz \
    -o /tmp/s6-overlay-arch.tar.xz
tar -C / -Jxpf /tmp/s6-overlay-arch.tar.xz
rm /tmp/s6-overlay-arch.tar.xz

apk del curl
EOF

ARG GIT_GID_UID=2001

RUN <<EOF
set -eux
addgroup -g ${GIT_GID_UID} git
adduser \
    -h /home/git \
    -s /usr/bin/josh-ssh-shell \
    -G git \
    -D \
    -u ${GIT_GID_UID} \
    git

# https://unix.stackexchange.com/a/193131/336647
usermod -p '*' git
EOF

COPY docker/etc/ssh/sshd_config.template /etc/ssh/

ARG RC6_D=/etc/s6-overlay/s6-rc.d

COPY docker/josh-auth-key docker/josh-ensure-* /opt/josh-scripts/
COPY docker/s6-rc.d/. ${RC6_D}/
COPY docker/finish ${RC6_D}/josh/
COPY docker/finish ${RC6_D}/sshd/

WORKDIR /
ENV S6_KEEP_ENV=1
ENV S6_BEHAVIOUR_IF_STAGE2_FAILS=2
ENV PATH=${PATH}:/command:/opt/josh-scripts
ENTRYPOINT ["/init"]
