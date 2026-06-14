# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-rust-dev-local
FROM ${BASE_IMAGE}

# Devtools needs these Alpine graphics libraries and build tools at link time.
RUN apk add --no-cache \
    libxkbcommon-dev \
    wayland-dev \
    libxcb-dev \
    libx11-dev \
    mesa-dev \
    vulkan-loader-dev \
    fontconfig-dev \
    pkgconf \
    perl \
    make
