# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

ARG ALPINE_VERSION=3.22.0
ARG BUILDER_IMAGE

FROM ${BUILDER_IMAGE} AS graphql-builder

RUN apk add --no-cache \
    openssl-dev \
    openssl-libs-static \
    zlib-dev \
    zlib-static \
    curl-dev \
    curl

RUN cargo install --verbose --version 0.14.0 graphql_client_cli

# Update check: https://github.com/facebookincubator/scrut/releases
ARG SCRUT_VERSION=0.4.3
# sha1 of scrut-v${SCRUT_VERSION}-linux-x86_64.tar.gz from the GitHub release
ARG SCRUT_SHA1=e3048b47f9bc4a6ec2d14ff9b5b05befe679521c
ARG TARGETARCH
RUN <<EOF
set -eux

# Install to /usr/local/cargo/bin so the "COPY --from=graphql-builder" in the
# final stage below works unchanged.
if [ "$TARGETARCH" = amd64 ]; then
    tarball=scrut-v${SCRUT_VERSION}-linux-x86_64.tar.gz
    curl -fsSL -o /tmp/${tarball} \
        https://github.com/facebookincubator/scrut/releases/download/v${SCRUT_VERSION}/${tarball}
    echo "${SCRUT_SHA1}  /tmp/${tarball}" | sha1sum -c -
    tar -xzf /tmp/${tarball} -C /tmp
    install -m 0755 /tmp/scrut-linux-x86_64/scrut /usr/local/cargo/bin/scrut
    rm -rf /tmp/${tarball} /tmp/scrut-linux-x86_64
else
    # The upstream aarch64 release tarball is mis-packaged (it contains
    # an x86-64 binary), so build from source on other architectures.
    cargo install --verbose --version ${SCRUT_VERSION} scrut
fi
EOF

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
    psmisc \
    zlib-dev \
    openssl-dev \
    curl-dev \
    openssh-client \
    patch \
    go \
    gcompat \
    libgcc

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

# On amd64, gcompat + libgcc (installed above) are needed to run the
# prebuilt, glibc-linked scrut binary on musl-based Alpine.
COPY --from=graphql-builder /usr/local/cargo/bin/scrut /usr/local/bin/scrut

WORKDIR /usr/src/josh

COPY lfs-test-server lfs-test-server
RUN cd lfs-test-server && GOPATH=/opt/git-lfs go install

ENV PATH=${PATH}:/opt/git-lfs/bin

ARG GIT_LFS_SHA=b84b33847fe6458f36ef521534dc0eac953cb379
RUN <<EOF
set -eux
git clone https://github.com/git-lfs/git-lfs.git /usr/src/git-lfs
cd /usr/src/git-lfs
git checkout "${GIT_LFS_SHA}"
make
cp bin/git-lfs /opt/git-lfs/bin
EOF

COPY --from=graphql-builder /usr/local/cargo/bin/graphql-client /usr/local/bin/graphql-client
