FROM rust:1.43-stretch as builder

RUN apt-get update \
 && apt-get install -y cmake python3-pip tree
RUN pip3 install cram

# RUN USER=root cargo new --bin /usr/src/josh
WORKDIR /usr/src/josh

# COPY ./Cargo.lock ./Cargo.lock
# COPY ./Cargo.toml ./Cargo.toml
# COPY ./prebuild.rs ./build.rs
#
# RUN cargo build --release && rm src/*.rs 

COPY . .

RUN cargo build --all
RUN git config --global user.email "josh@test.com"
RUN git config --global user.name "Josh Mac Testington the third"

# RUN cram ./tests/filter/*.t

RUN rm ./target/release/deps/josh* && cargo build --release
RUN cargo build -p josh-proxy

FROM rust:1.43-stretch

COPY --from=builder /usr/src/josh/target/debug/josh-proxy /usr/bin/josh-proxy
COPY --from=builder /usr/src/josh/run-josh.sh /usr/bin/run-josh.sh

CMD sh /usr/bin/run-josh.sh
