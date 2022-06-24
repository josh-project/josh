# syntax=docker/dockerfile:1.4-labs

ARG RUST_VERSION=1.61.0

FROM rust:${RUST_VERSION} as dev-planner

RUN cargo install --version 0.1.35 cargo-chef

WORKDIR /usr/src/josh
COPY . .

ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:${RUST_VERSION} as dev

RUN <<EOF
apt-get update
apt-get remove --yes git
apt-get install --yes --no-install-recommends \
    cmake \
    gcc \
    make \
    libz-dev \
    libssl-dev \
    libcurl4-openssl-dev \
    libexpat1-dev \
    gettext \
    python3 \
    python3-pip \
    tree \
    psmisc
rm -rf /var/lib/apt/lists/*
EOF

ARG GIT_VERSION=2.36.1
WORKDIR /usr/src/git
RUN <<EOF
wget https://mirrors.edge.kernel.org/pub/software/scm/git/git-${GIT_VERSION}.tar.gz
tar --extract --gzip --file git-${GIT_VERSION}.tar.gz
cd git-${GIT_VERSION}
make configure
./configure --prefix=/opt/git-install --exec-prefix=/opt/git-install
make -j$(nproc)
make install
EOF

RUN mkdir /opt/git-install/etc

ENV PATH=${PATH}:/opt/git-install/bin
RUN git config -f /opt/git-install/etc/gitconfig --add safe.directory "*"

ARG CRAM_VERSION=d245cca
ARG PYGIT2_VERSION=1.9.1
RUN pip3 install \
  git+https://github.com/brodie/cram.git@${CRAM_VERSION} \
  pygit2==${PYGIT2_VERSION}

RUN <<EOF
curl --fail --show-error --silent --location https://deb.nodesource.com/setup_17.x | bash - && \
apt-get install --yes nodejs
EOF

WORKDIR /usr/src/josh
RUN rustup component add rustfmt
RUN cargo install --version 0.1.35 cargo-chef
RUN cargo install --version 0.2.1 hyper_cgi --features=test-server
RUN cargo install --version 0.10.0 graphql_client_cli


FROM dev as dev-local

RUN mkdir -p /opt/cache && \
    chmod 777 /opt/cache

VOLUME /opt/cache

ENV CARGO_TARGET_DIR=/opt/cache/cargo-target
ENV CARGO_HOME=/opt/cache/cargo-cache
RUN npm config set cache /opt/cache/npm-cache --global

FROM dev as dev-ci

COPY --from=dev-planner /usr/src/josh/recipe.json .
ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN cargo chef cook --workspace --recipe-path recipe.json

RUN mkdir -p josh-ui
COPY josh-ui/package.json josh-ui/package-lock.json josh-ui/
RUN cd josh-ui && npm install

FROM dev as build


COPY . .
RUN \
  cargo build -p josh-proxy --release && \
  cargo build -p josh-ui --release

FROM debian:bullseye as run

RUN <<EOF
apt-get update
apt-get install --yes --no-install-recommends \
    zlib1g \
    libexpat1 \
    libcurl4 \
    ca-certificates
rm -rf /var/lib/apt/lists/*
EOF

COPY --from=dev --link=false /opt/git-install /opt/git-install
ENV PATH=${PATH}:/opt/git-install/bin

COPY --from=build --link=false /usr/src/josh/target/release/josh-proxy /usr/bin/
COPY --from=build --link=false /usr/src/josh/run-josh.sh /usr/bin/
COPY --from=build --link=false /usr/src/josh/static/ /josh/static/

CMD sh /usr/bin/run-josh.sh
