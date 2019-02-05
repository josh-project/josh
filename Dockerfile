FROM rust:1.32.0 as builder

RUN apt-get update \
 && apt-get install -y cmake \
 && rm -rf /var/lib/apt/lists/*

# RUN USER=root cargo new --bin /usr/src/josh
WORKDIR /usr/src/josh

# COPY ./Cargo.lock ./Cargo.lock
# COPY ./Cargo.toml ./Cargo.toml
# COPY ./prebuild.rs ./build.rs
#
# RUN cargo build --release && rm src/*.rs 

COPY . .

# RUN rm ./target/release/deps/josh* && cargo build --release
RUN cargo build

FROM rust:1.32.0

COPY --from=builder /usr/src/josh/target/debug/josh-proxy /usr/bin/josh-proxy

CMD RUST_BACKTRACE=1 josh-proxy --local=/tmp/josh-scratch/ --remote=https://gerrit.int.esrlabs.com
