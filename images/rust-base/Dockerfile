# syntax=docker/dockerfile:1.15.1@sha256:9857836c9ee4268391bb5b09f9f157f3c91bb15821bb77969642813b0d00518d

ARG ALPINE_VERSION=3.22.0
ARG ARCH

FROM alpine:${ALPINE_VERSION}

RUN apk add --no-cache ca-certificates gcc musl-dev

ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:${PATH}

ARG ARCH

# Update check: https://github.com/rust-lang/rustup/tags
ARG RUSTUP_VERSION=1.28.2

# Update check: https://github.com/rust-lang/rust/tags
ARG RUST_VERSION=1.92.0

# https://github.com/sfackler/rust-openssl/issues/1462
ENV RUSTFLAGS="-Ctarget-feature=-crt-static"

RUN <<EOF
set -eux

apk add --no-cache curl

if [ "$ARCH" = amd64 ]; then
    rust_arch=x86_64;
elif [ "$ARCH" = arm64 ]; then
    rust_arch=aarch64;
else
    echo "Unsupported arch";
    exit 1
fi

rust_arch=${rust_arch}-unknown-linux-musl

curl -sSL https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${rust_arch}/rustup-init -o /tmp/rustup-init
chmod +x /tmp/rustup-init
/tmp/rustup-init \
    -y \
    --no-modify-path \
    --profile minimal \
    --default-toolchain ${RUST_VERSION} \
    --default-host ${rust_arch}
rm /tmp/rustup-init
apk del curl
EOF
