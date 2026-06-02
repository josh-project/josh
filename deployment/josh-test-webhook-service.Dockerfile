# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG ALPINE_VERSION=3.22

# Update check: https://github.com/rust-lang/rust/tags
ARG RUST_VERSION=1.92.0

FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS build

RUN apk add --no-cache musl-dev

WORKDIR /src

# Bind-mount the source (read-only) instead of copying it into a layer. The
# target dir is a persistent cache volume so subsequent reruns reuse compiled
# artifacts; neither the source bind nor the cache mounts are part of the image
# layer, so the binary is copied out within the same RUN.
ENV CARGO_TARGET_DIR=/opt/cargo-target
ENV CARGO_INCREMENTAL=0
RUN --mount=type=bind,target=/src \
    --mount=type=cache,id=josh-cargo-cache,target=/opt/cargo-target \
    --mount=type=cache,id=josh-cargo-registry,target=/usr/local/cargo/registry \
    set -eux; \
    cargo build -p josh-test-webhook-service --release; \
    cp /opt/cargo-target/release/josh-test-webhook-service /usr/local/bin/

FROM alpine:${ALPINE_VERSION} AS release

RUN apk add --no-cache ca-certificates

COPY --from=build /usr/local/bin/josh-test-webhook-service /usr/bin/

EXPOSE 3000

ENTRYPOINT ["/usr/bin/josh-test-webhook-service"]
