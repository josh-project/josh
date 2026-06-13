# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-rust-dev-local
FROM ${BASE_IMAGE}

# Dioxus desktop needs these Linux libraries and pkg-config metadata at link time.
RUN apk add --no-cache \
    webkit2gtk-4.1-dev \
    libsoup3-dev \
    glib-dev \
    xdotool-dev \
    pkgconf
