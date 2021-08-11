FROM rust:1.54 as builder

RUN apt-get update \
 && apt-get install -y cmake \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/josh
COPY . .

RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli
RUN cargo install trunk
RUN trunk --config=josh-ui/Trunk.toml build
RUN cargo build -p josh-proxy

FROM rust:1.54

COPY --from=builder /usr/src/josh/target/debug/josh-proxy /usr/bin/josh-proxy
COPY --from=builder /usr/src/josh/run-josh.sh /usr/bin/run-josh.sh
COPY --from=builder /usr/src/josh/static/ /josh/static/

CMD sh /usr/bin/run-josh.sh
