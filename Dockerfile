# syntax = docker/dockerfile:1.22
########################################

FROM rust:1.98-trixie AS builder
ARG TARGETARCH

RUN apt-get update && \
    apt-get install -y build-essential make cmake pkg-config libssl-dev musl-dev musl-tools

RUN case "${TARGETARCH}" in \
        amd64) \
            apt-get install -y --no-install-recommends gcc-x86-64-linux-gnu && \
            rustup target add x86_64-unknown-linux-gnu; \
            rustup target add x86_64-unknown-linux-musl; \
            ;; \
        arm64) \
            apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu && \
            rustup target add aarch64-unknown-linux-gnu; \
            rustup target add aarch64-unknown-linux-musl; \
            ;; \
        *) echo "Unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac
RUN rustup component add rustfmt clippy && \
    cargo install cargo-deny

RUN ARCH=$(case "${TARGETARCH}" in \
        amd64) echo "x86_64" ;; \
        arm64) echo "aarch64" ;; \
        *) echo "${TARGETARCH}" ;; \
    esac) && curl -LO https://ziglang.org/download/0.16.0/zig-${ARCH}-linux-0.16.0.tar.xz \
    && tar -xf zig-${ARCH}-linux-0.16.0.tar.xz -C /opt \
    && ln -s /opt/zig-${ARCH}-linux-0.16.0/zig /usr/local/bin/zig

RUN echo 'deb [trusted=yes] https://repo.goreleaser.com/apt/ /' | tee /etc/apt/sources.list.d/goreleaser.list && \
    apt-get update && apt-get install -y goreleaser

RUN cargo install cargo-binstall --locked && \
    cargo binstall cargo-zigbuild --no-confirm

COPY [".", "/src"]

########################################

FROM --platform=${TARGETARCH} gcr.io/distroless/cc-debian13:nonroot AS release

ARG TARGETPLATFORM
COPY ${TARGETPLATFORM}/aralez /usr/local/bin/aralez

ENTRYPOINT ["/usr/local/bin/aralez"]
