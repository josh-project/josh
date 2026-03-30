# syntax=docker/dockerfile:1.15.1@sha256:9857836c9ee4268391bb5b09f9f157f3c91bb15821bb77969642813b0d00518d

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
