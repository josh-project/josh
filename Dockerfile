FROM rust:1.51 as builder

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
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli
RUN cargo install trunk
RUN trunk --config=josh-ui/Trunk.toml build
RUN cargo build -p josh-proxy

FROM rust:1.51

COPY --from=builder /usr/src/josh/target/debug/josh-proxy /usr/bin/josh-proxy
COPY --from=builder /usr/src/josh/run-josh.sh /usr/bin/run-josh.sh
COPY --from=builder /usr/src/josh/static/ /josh/static/

CMD sh /usr/bin/run-josh.sh
