FROM docker.io/library/alpine:latest AS builder

RUN apk add --no-cache curl clang gcc musl-dev lld cmake make && \
    curl -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain nightly -y

ENV CC clang
ENV CFLAGS "-I/usr/lib/gcc/x86_64-alpine-linux-musl/9.3.0/ -L/usr/lib/gcc/x86_64-alpine-linux-musl/9.3.0/"
ENV RUSTFLAGS "-C link-arg=-fuse-ld=lld -C target-cpu=haswell"

RUN rm /usr/bin/ld && \
    rm /usr/bin/cc && \
    ln -s /usr/bin/lld /usr/bin/ld && \
    ln -s /usr/bin/clang /usr/bin/cc && \
    ln -s /usr/lib/gcc/x86_64-alpine-linux-musl/9.3.0/crtbeginS.o /usr/lib/crtbeginS.o && \
    ln -s /usr/lib/gcc/x86_64-alpine-linux-musl/9.3.0/crtendS.o /usr/lib/crtendS.o

WORKDIR /build

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
COPY ./.cargo ./.cargo

RUN mkdir src/
RUN echo 'fn main() {}' > ./src/main.rs
RUN source $HOME/.cargo/env && \
    cargo build --release

RUN rm -f target/release/deps/twilight_dispatch_andesite*

COPY ./src ./src

RUN source $HOME/.cargo/env && \
    cargo build --release && \
    strip /build/target/release/twilight-dispatch-andesite

FROM docker.io/library/alpine:latest

RUN adduser -S twilight-dispatch-andesite

USER twilight-dispatch-andesite
WORKDIR /twilight-dispatch-andesite

COPY --from=builder /build/target/release/twilight-dispatch-andesite /twilight-dispatch-andesite/run

CMD /twilight-dispatch-andesite/run
