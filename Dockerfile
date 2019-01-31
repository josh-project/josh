FROM rust:1.32.0 as builder

RUN apt-get update \
 && apt-get install -y cmake \
 && rm -rf /var/lib/apt/lists/*

# RUN USER=root cargo new --bin /usr/src/grib
WORKDIR /usr/src/grib

# COPY ./Cargo.lock ./Cargo.lock
# COPY ./Cargo.toml ./Cargo.toml
# COPY ./prebuild.rs ./build.rs
#
# RUN cargo build --release && rm src/*.rs 

COPY . .

# RUN rm ./target/release/deps/grib* && cargo build --release
RUN cargo build --release

FROM rust:1.32.0

COPY --from=builder /usr/src/grib/target/release/grib /usr/bin/grib

CMD grib --local=/tmp/grib-scratch/ --remote=https://gerrit.int.esrlabs.com
