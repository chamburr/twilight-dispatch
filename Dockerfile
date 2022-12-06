FROM docker.io/library/alpine:latest AS builder

ARG TARGET_CPU=haswell
ENV RUSTFLAGS "-Lnative=/usr/lib -Z mir-opt-level=3 -C target-cpu=${TARGET_CPU}"

RUN apk add --no-cache curl gcc g++ musl-dev cmake make && \
    curl -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain nightly -y

WORKDIR /build

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
COPY ./.cargo ./.cargo

RUN mkdir src/
RUN echo 'fn main() {}' > ./src/main.rs
RUN source $HOME/.cargo/env && \
    cargo build --release

RUN rm -f target/release/deps/twilight_dispatch*

COPY ./src ./src

RUN source $HOME/.cargo/env && \
    cargo build --release && \
    strip /build/target/release/twilight-dispatch

FROM docker.io/library/alpine:latest

RUN apk add --no-cache dumb-init

COPY --from=builder /build/target/release/twilight-dispatch /twilight-dispatch

ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["./twilight-dispatch"]
