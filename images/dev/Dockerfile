# syntax=docker/dockerfile:1.15.1@sha256:9857836c9ee4268391bb5b09f9f157f3c91bb15821bb77969642813b0d00518d

ARG BASE_IMAGE
FROM ws_image_${BASE_IMAGE}

RUN apk add --no-cache \
    zlib-static \
    zlib-dev \
    openssl-libs-static \
    openssl-dev \
    curl-dev

WORKDIR /usr/src/josh
RUN rustup component add rustfmt
RUN cargo install --verbose --version 0.14.0 graphql_client_cli

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
    psmisc

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

RUN apk add --no-cache go nodejs npm openssh-client patch

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

WORKDIR /usr/src/josh

# Update check: https://github.com/mozilla/sccache/releases
ARG SCCACHE_VERSION=0.14.0
ARG ARCH
RUN <<EOF
set -eux

if [ "$ARCH" = amd64 ]; then
    sccache_arch=x86_64;
elif [ "$ARCH" = arm64 ]; then
    sccache_arch=aarch64;
else
    echo "Unsupported arch";
    exit 1
fi

curl -sSL https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-${sccache_arch}-unknown-linux-musl.tar.gz \
    -o /tmp/sccache.tar.gz
tar -xzf /tmp/sccache.tar.gz -C /tmp
mv /tmp/sccache-v${SCCACHE_VERSION}-${sccache_arch}-unknown-linux-musl/sccache /usr/local/bin/sccache
chmod +x /usr/local/bin/sccache
rm -rf /tmp/sccache*
EOF
