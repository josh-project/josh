# syntax=docker/dockerfile:1.4-labs

ARG ALPINE_VERSION=3.17

FROM alpine:${ALPINE_VERSION} as rust-base

RUN apk add --no-cache ca-certificates gcc musl-dev

ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:${PATH}

ARG ARCH=x86_64
ARG RUSTUP_VERSION=1.25.1
ARG RUST_VERSION=1.66.0
ARG RUST_ARCH=${ARCH}-unknown-linux-musl

# https://github.com/sfackler/rust-openssl/issues/1462
ENV RUSTFLAGS="-Ctarget-feature=-crt-static"

ADD --chmod=755 https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${RUST_ARCH}/rustup-init /tmp
RUN /tmp/rustup-init \
    -y \
    --no-modify-path \
    --profile minimal \
    --default-toolchain ${RUST_VERSION} \
    --default-host ${RUST_ARCH}

FROM rust-base as dev-planner

RUN cargo install --version 0.1.51 cargo-chef

WORKDIR /usr/src/josh
COPY . .

ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN cargo chef prepare --recipe-path recipe.json

FROM rust-base as dev

RUN apk add --no-cache \
    zlib-dev \
    openssl-dev \
    curl-dev

WORKDIR /usr/src/josh
RUN rustup component add rustfmt
RUN cargo install --version 0.1.51 cargo-chef
RUN cargo install --verbose --version 0.10.0 graphql_client_cli

RUN apk add --no-cache \
    bash \
    curl \
    cmake \
    make \
    expat-dev \
    gettext \
    python3 \
    python3-dev \
    py3-pip \
    tree \
    autoconf \
    libgit2-dev \
    psmisc

ARG GIT_VERSION=2.38.1
WORKDIR /usr/src/git
RUN <<EOF
set -e
wget https://mirrors.edge.kernel.org/pub/software/scm/git/git-${GIT_VERSION}.tar.gz
tar --extract --gzip --file git-${GIT_VERSION}.tar.gz
cd git-${GIT_VERSION}
make configure
./configure \
    --without-tcltk \
    --prefix=/opt/git-install \
    --exec-prefix=/opt/git-install
make -j$(nproc)
make install
EOF

ENV PATH=${PATH}:/opt/git-install/bin
RUN mkdir /opt/git-install/etc
RUN git config -f /opt/git-install/etc/gitconfig --add safe.directory "*" && \
    git config -f /opt/git-install/etc/gitconfig protocol.file.allow "always"

ARG CRAM_VERSION=d245cca
ARG PYGIT2_VERSION=1.11.1
RUN pip3 install \
  git+https://github.com/brodie/cram.git@${CRAM_VERSION}

RUN apk add --no-cache go nodejs npm openssh-client patch

ARG GIT_LFS_VERSION=d4ced458b5cc9eaa712c1a2d299d77a4e3a0a7c5
RUN GOPATH=/opt/lfs-test-server go install \
    github.com/git-lfs/lfs-test-server@${GIT_LFS_VERSION}
ENV PATH=${PATH}:/opt/lfs-test-server/bin

WORKDIR /usr/src/josh

FROM dev as dev-local

RUN mkdir -p /opt/cache && \
    chmod 777 /opt/cache

RUN mkdir -p /josh/static && \
    chmod 777 /josh/static

VOLUME /opt/cache

ENV CARGO_TARGET_DIR=/opt/cache/cargo-target
ENV CARGO_HOME=/opt/cache/cargo-cache
ENV GOCACHE=/opt/cache/go-cache
ENV GOPATH=/opt/cache/go-path
RUN npm config set cache /opt/cache/npm-cache --global

ARG USER_GID
ARG USER_UID

RUN \
  if [ ! $(getent group ${USER_GID}) ] ; then \
    addgroup \
      -g ${USER_GID} dev ; \
  fi

RUN adduser \
      -u ${USER_UID} \
      -G $(getent group ${USER_GID} | cut -d: -f1) \
      -D \
      -H \
      -g '' \
      dev

FROM dev as dev-cache

COPY --from=dev-planner /usr/src/josh/recipe.json .
ENV CARGO_TARGET_DIR=/opt/cargo-target

FROM dev-cache as dev-ci

RUN mkdir -p /josh/static && \
    chmod 777 /josh/static

RUN cargo chef cook --workspace --recipe-path recipe.json

RUN mkdir -p josh-ui
COPY josh-ui/package.json josh-ui/package-lock.json josh-ui/
RUN cd josh-ui && npm install

FROM dev-cache as build

RUN cargo chef cook --release --workspace --recipe-path recipe.json

COPY Cargo.toml Cargo.lock josh-ui josh-ui/
RUN cargo build -p josh-ui --release
COPY . .
RUN --mount=target=.git,from=git \
  cargo build -p josh-proxy -p josh-ssh-shell --release

ARG ALPINE_VERSION
FROM alpine:${ALPINE_VERSION} as run

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
ADD https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-noarch.tar.xz /tmp
RUN tar -C / -Jxpf /tmp/s6-overlay-noarch.tar.xz
ADD https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-${ARCH}.tar.xz /tmp
RUN tar -C / -Jxpf /tmp/s6-overlay-${ARCH}.tar.xz

ARG GIT_GID_UID=2001

RUN addgroup -g ${GIT_GID_UID} git
RUN adduser \
    -h /home/git \
    -s /usr/bin/josh-ssh-shell \
    -G git \
    -D \
    -u ${GIT_GID_UID} \
    git

# https://unix.stackexchange.com/a/193131/336647
RUN usermod -p '*' git

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
