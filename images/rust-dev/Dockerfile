# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-rust-base
FROM ${BASE_IMAGE}

RUN apk add --no-cache \
    bash \
    git \
    git-daemon \
    zlib-static \
    zlib-dev \
    openssl-libs-static \
    openssl-dev \
    curl-dev

RUN rustup component add rustfmt

# Update check: https://github.com/mozilla/sccache/releases
ARG SCCACHE_VERSION=0.14.0
ARG TARGETARCH
RUN <<EOF
set -eux

apk add --no-cache curl

if [ "$TARGETARCH" = amd64 ]; then
    sccache_arch=x86_64;
elif [ "$TARGETARCH" = arm64 ]; then
    sccache_arch=aarch64;
else
    echo "Unsupported arch";
    exit 1
fi

curl -sSL https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-${sccache_arch}-unknown-linux-musl.tar.gz \
    -o /tmp/sccache.tar.gz
tar -xzf /tmp/sccache.tar.gz -C /tmp
mv /tmp/sccache-v${SCCACHE_VERSION}-${sccache_arch}-unknown-linux-musl/sccache /usr/local/bin/sccache
chmod +x /usr/local/bin/sccache
rm -rf /tmp/sccache*
apk del curl
EOF
