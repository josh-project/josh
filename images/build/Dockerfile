# syntax=docker/dockerfile:1.15.1@sha256:9857836c9ee4268391bb5b09f9f157f3c91bb15821bb77969642813b0d00518d

ARG BASE_IMAGE=josh-dev
FROM ${BASE_IMAGE}

ENV CARGO_TARGET_DIR=/opt/cargo-target

COPY . .
RUN --mount=target=.git,from=git \
  cargo build -p josh-ui --release && \
  cargo build -p josh-proxy -p josh-ssh-shell --release
