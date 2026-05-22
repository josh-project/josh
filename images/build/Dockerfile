# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-dev
FROM ${BASE_IMAGE}

ENV CARGO_TARGET_DIR=/opt/cargo-target
ENV CARGO_INCREMENTAL=0

COPY . .
ARG JOSH_VERSION=unknown
RUN JOSH_VERSION=${JOSH_VERSION} cargo build -p josh-proxy -p josh-ssh-shell --release
