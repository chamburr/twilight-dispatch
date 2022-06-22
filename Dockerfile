FROM docker.io/library/alpine:latest AS builder

RUN apk add --no-cache curl clang gcc musl-dev lld cmake make && \
    curl -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain nightly -y

ENV CC clang
ENV CFLAGS "-I/usr/lib/gcc/x86_64-alpine-linux-musl/11.2.1 -L/usr/lib/gcc/x86_64-alpine-linux-musl/11.2.1/"
ENV RUSTFLAGS "-C link-arg=-fuse-ld=lld -C target-cpu=haswell"

RUN rm /usr/bin/ld && \
    rm /usr/bin/cc && \
    ln -s /usr/bin/lld /usr/bin/ld && \
    ln -s /usr/bin/clang /usr/bin/cc && \
    ln -s /usr/lib/gcc/x86_64-alpine-linux-musl/11.2.1/crtbeginS.o /usr/lib/crtbeginS.o && \
    ln -s /usr/lib/gcc/x86_64-alpine-linux-musl/11.2.1/crtendS.o /usr/lib/crtendS.o

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

FROM docker.io/library/alpine:edge AS dumb-init

RUN apk update && \
    VERSION=$(apk search dumb-init) && \
    mkdir out && \
    cd out && \
    wget "https://dl-cdn.alpinelinux.org/alpine/edge/community/x86_64/$VERSION.apk" -O dumb-init.apk && \
    tar xf dumb-init.apk && \
    mv usr/bin/dumb-init /dumb-init

FROM scratch

COPY --from=builder /build/target/release/twilight-dispatch /twilight-dispatch
COPY --from=dumb-init /dumb-init /dumb-init

ENTRYPOINT ["./dumb-init", "--"]
CMD ["./twilight-dispatch"]
