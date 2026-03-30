# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-dev
FROM ${BASE_IMAGE}

ENV RUSTC_WRAPPER=sccache
ENV CARGO_INCREMENTAL=0
ENV CARGO_TARGET_DIR=/opt/cargo-target

RUN mkdir -p /josh/static && \
    chmod 777 /josh/static

RUN mkdir -p josh-ui
COPY josh-ui/package.json josh-ui/package-lock.json josh-ui/
RUN cd josh-ui && npm install
