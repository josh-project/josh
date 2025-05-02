# syntax=docker/dockerfile:1.8@sha256:d6d396f3780b1dd56a3acbc975f57bd2fc501989b50164c41387c42d04e780d0

ARG ALPINE_VERSION=3.21
ARG ARCH=${TARGETARCH}

FROM alpine:${ALPINE_VERSION} AS rust-base

RUN apk add --no-cache ca-certificates gcc musl-dev

ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:${PATH}

ARG ARCH

# Update check: https://github.com/rust-lang/rustup/tags
ARG RUSTUP_VERSION=1.27.1

# Update check: https://github.com/rust-lang/rust/tags
ARG RUST_VERSION=1.85.0

# https://github.com/sfackler/rust-openssl/issues/1462
ENV RUSTFLAGS="-Ctarget-feature=-crt-static"

RUN <<EOF
set -eux

apk add --no-cache curl

if [ "$ARCH" = amd64 ]; then
    rust_arch=x86_64;
elif [ "$ARCH" = arm64 ]; then
    rust_arch=aarch64;
else
    echo "Unsupported arch";
    exit 1
fi

rust_arch=${rust_arch}-unknown-linux-musl

curl -sSL https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${rust_arch}/rustup-init -o /tmp/rustup-init
chmod +x /tmp/rustup-init
/tmp/rustup-init \
    -y \
    --no-modify-path \
    --profile minimal \
    --default-toolchain ${RUST_VERSION} \
    --default-host ${rust_arch}
rm /tmp/rustup-init
apk del curl
EOF

FROM rust-base AS dev-planner

# Update check: https://github.com/LukeMathWalker/cargo-chef/releases
RUN cargo install --version 0.1.71 cargo-chef

WORKDIR /usr/src/josh
COPY . .

ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN cargo chef prepare --recipe-path recipe.json

FROM rust-base AS dev

RUN apk add --no-cache \
    zlib-dev \
    openssl-dev \
    curl-dev

WORKDIR /usr/src/josh
RUN rustup component add rustfmt
RUN cargo install --version 0.1.71 cargo-chef &&\
    cargo install --verbose --version 0.10.0 graphql_client_cli

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
ARG GIT_VERSION=2.45.2
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

ARG GIT_LFS_TEST_SERVER_VERSION=d4ced458b5cc9eaa712c1a2d299d77a4e3a0a7c5

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

FROM dev AS dev-local

RUN <<EOF
set -eux
mkdir -p /opt/cache /josh/static
chmod 777 /opt/cache /josh/static
EOF

VOLUME /opt/cache

ENV CARGO_TARGET_DIR=/opt/cache/cargo-target
ENV CARGO_HOME=/opt/cache/cargo-cache
ENV GOCACHE=/opt/cache/go-cache
ENV GOPATH=/opt/cache/go-path
ENV GOFLAGS=-buildvcs=false
RUN npm config set cache /opt/cache/npm-cache --global

ARG USER_GID
ARG USER_UID

RUN <<EOF
set -eux

if [ ! $(getent group ${USER_GID}) ] ; then
    addgroup -g ${USER_GID} dev
fi

adduser \
    -u ${USER_UID} \
    -G $(getent group ${USER_GID} | cut -d: -f1) \
    -D \
    -H \
    -g '' \
    dev
EOF

FROM dev AS dev-cache

COPY --from=dev-planner /usr/src/josh/recipe.json .
ENV CARGO_TARGET_DIR=/opt/cargo-target

FROM dev-cache AS dev-ci

RUN mkdir -p /josh/static && \
    chmod 777 /josh/static

RUN cargo chef cook --workspace --recipe-path recipe.json

RUN mkdir -p josh-ui
COPY josh-ui/package.json josh-ui/package-lock.json josh-ui/
RUN cd josh-ui && npm install

FROM dev-cache AS build

RUN cargo chef cook --release --workspace --recipe-path recipe.json

COPY Cargo.toml Cargo.lock josh-ui josh-ui/
RUN cargo build -p josh-ui --release
COPY . .
RUN --mount=target=.git,from=git \
  cargo build -p josh-proxy -p josh-ssh-shell --release

ARG ALPINE_VERSION
FROM alpine:${ALPINE_VERSION} AS run

RUN apk add --no-cache \
    zlib \
    openssl \
    libexpat \
    libgit2 \
    libgcc \
    libcurl \
    ca-certificates \
    openssh \
    bash \
    xz \
    shadow \
    gettext

COPY --from=dev --link=false /opt/git-install /opt/git-install
ENV PATH=${PATH}:/opt/git-install/bin

COPY --from=build --link=false /opt/cargo-target/release/josh-proxy /usr/bin/
COPY --from=build --link=false /opt/cargo-target/release/josh-ssh-shell /usr/bin/
COPY --from=build --link=false /usr/src/josh/static/ /josh/static/

ARG S6_OVERLAY_VERSION=3.1.2.1
ARG ARCH
RUN <<EOF
set -eux

apk add --no-cache curl

curl -sSL https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-noarch.tar.xz \
    -o /tmp/s6-overlay-noarch.tar.xz
tar -C / -Jxpf /tmp/s6-overlay-noarch.tar.xz
rm /tmp/s6-overlay-noarch.tar.xz

if [ "$ARCH" = amd64 ]; then
    s6_arch=x86_64;
elif [ "$ARCH" = arm64 ]; then
    s6_arch=aarch64;
else
    echo "Unsupported arch";
    exit 1
fi

curl -sSL https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-${s6_arch}.tar.xz \
    -o /tmp/s6-overlay-arch.tar.xz
tar -C / -Jxpf /tmp/s6-overlay-arch.tar.xz
rm /tmp/s6-overlay-arch.tar.xz

apk del curl
EOF

ARG GIT_GID_UID=2001

RUN <<EOF
set -eux
addgroup -g ${GIT_GID_UID} git
adduser \
    -h /home/git \
    -s /usr/bin/josh-ssh-shell \
    -G git \
    -D \
    -u ${GIT_GID_UID} \
    git

# https://unix.stackexchange.com/a/193131/336647
usermod -p '*' git
EOF

COPY --from=docker --link=false etc/ssh/sshd_config.template /etc/ssh/sshd_config.template

ARG RC6_D=/etc/s6-overlay/s6-rc.d

COPY --from=docker --link=false \
  josh-auth-key \
  josh-ensure-dir \
  josh-ensure-mode \
  josh-ensure-owner \
  /opt/josh-scripts/
COPY --from=docker --link=false s6-rc.d/. ${RC6_D}/
COPY --from=docker --link=false finish ${RC6_D}/josh/
COPY --from=docker --link=false finish ${RC6_D}/sshd/

WORKDIR /
ENV S6_KEEP_ENV=1
ENV S6_BEHAVIOUR_IF_STAGE2_FAILS=2
ENV PATH=${PATH}:/opt/josh-scripts
ENTRYPOINT ["/init"]
