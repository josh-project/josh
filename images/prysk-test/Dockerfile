# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG ALPINE_VERSION=3.22.0
ARG BUILDER_IMAGE

FROM ${BUILDER_IMAGE} AS graphql-builder

RUN apk add --no-cache \
    openssl-dev \
    openssl-libs-static \
    zlib-dev \
    zlib-static \
    curl-dev

RUN cargo install --verbose --version 0.14.0 graphql_client_cli

FROM alpine:${ALPINE_VERSION}

RUN apk add --no-cache \
    bash \
    coreutils \
    curl \
    cmake \
    make \
    expat-dev \
    gettext \
    python3 \
    python3-dev \
    libffi-dev \
    py3-pip \
    tree \
    autoconf \
    libgit2-dev \
    psmisc \
    zlib-dev \
    openssl-dev \
    curl-dev \
    openssh-client \
    patch \
    go

# Update check: https://github.com/git/git/tags
ARG GIT_VERSION=2.50.0
ENV PATH=${PATH}:/opt/git-install/bin
WORKDIR /usr/src/git
RUN <<EOF
set -e
wget https://mirrors.edge.kernel.org/pub/software/scm/git/git-${GIT_VERSION}.tar.gz
tar --extract --gzip --file git-${GIT_VERSION}.tar.gz
rm git-${GIT_VERSION}.tar.gz
cd git-${GIT_VERSION}
make configure
./configure \
    --without-tcltk \
    --prefix=/opt/git-install \
    --exec-prefix=/opt/git-install
make -j$(nproc)
make install
mkdir /opt/git-install/etc
git config -f /opt/git-install/etc/gitconfig --add safe.directory "*"
git config -f /opt/git-install/etc/gitconfig protocol.file.allow "always"
EOF

# Update check: https://github.com/prysk/prysk/releases
ARG PRYSK_VERSION=0.20.0

# This is a Docker image so --break-system-packages is okay
RUN pip3 install --break-system-packages \
  git+https://github.com/prysk/prysk.git@${PRYSK_VERSION}

WORKDIR /usr/src/josh

COPY lfs-test-server lfs-test-server
RUN cd lfs-test-server && GOPATH=/opt/git-lfs go install

ENV PATH=${PATH}:/opt/git-lfs/bin

RUN <<EOF
set -eux
git clone https://github.com/git-lfs/git-lfs.git /usr/src/git-lfs
cd /usr/src/git-lfs
make
cp bin/git-lfs /opt/git-lfs/bin
EOF

COPY --from=graphql-builder /usr/local/cargo/bin/graphql-client /usr/local/bin/graphql-client
