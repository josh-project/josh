FROM rust:1.58.1 as builder

RUN apt-get update \
 && apt-get install -y cmake \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/josh
RUN rustup target add wasm32-unknown-unknown
RUN cargo install --version 0.2.78 wasm-bindgen-cli
RUN cargo install --version 0.14.0 trunk

COPY . .
RUN trunk --config=josh-ui/Trunk.toml build
RUN cargo build -p josh-proxy --release

FROM rust:1.58.1

COPY --from=builder /usr/src/josh/target/release/josh-proxy /usr/bin/josh-proxy
COPY --from=builder /usr/src/josh/run-josh.sh /usr/bin/run-josh.sh
COPY --from=builder /usr/src/josh/static/ /josh/static/

CMD sh /usr/bin/run-josh.sh
