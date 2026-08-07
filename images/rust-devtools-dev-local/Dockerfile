# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG BASE_IMAGE=josh-rust-dev-local
FROM ${BASE_IMAGE}

# devtools/git-tree-viewer pulls in eframe + wgpu + winit, which on Alpine
# Linux need xkbcommon / wayland / X11 / mesa / vulkan dev libs to link at
# build time and pkgconf to discover them. Kept separate from
# rust-gui-dev-local so josh-gui jobs don't pull in mesa/vulkan and devtools
# jobs don't pull in webkit/libsoup.
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
