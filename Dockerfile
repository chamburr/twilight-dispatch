FROM rust:1.65-alpine AS builder

ENV RUSTFLAGS="-C target-cpu=haswell"

RUN apk add --no-cache gcc g++ musl-dev cmake make

WORKDIR /build

COPY .cargo ./.cargo
COPY Cargo.toml Cargo.lock ./

RUN mkdir src
RUN echo 'fn main() {}' > ./src/main.rs
RUN cargo build --release
RUN rm -f target/release/deps/twilight_dispatch*

COPY src ./src

RUN cargo build --release

FROM alpine:3.17

RUN apk add --no-cache dumb-init

WORKDIR /app

COPY --from=builder /build/target/release/twilight-dispatch ./

EXPOSE 8005

ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["./twilight-dispatch"]
