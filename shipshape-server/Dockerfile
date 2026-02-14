# syntax=docker/dockerfile:1.7

FROM rust:1.76-bullseye AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY shipshape-cli/Cargo.toml shipshape-cli/Cargo.toml
COPY shipshape-core/Cargo.toml shipshape-core/Cargo.toml
COPY shipshape-server/Cargo.toml shipshape-server/Cargo.toml

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo fetch -p shipshape-server

COPY shipshape-cli ./shipshape-cli
COPY shipshape-core ./shipshape-core
COPY shipshape-server ./shipshape-server

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p shipshape-server

FROM debian:bullseye-slim AS runtime
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        git \
        golang-go \
        python3 \
        python3-pip \
    && rm -rf /var/lib/apt/lists/*

ARG SHIPSHAPE_PIP_LIBS="cdd-c type-correct lib2notebook2lib go-auto-err-handling"
RUN python3 -m pip install --no-cache-dir --upgrade pip \
    && python3 -m pip install --no-cache-dir ${SHIPSHAPE_PIP_LIBS}

ENV PATH="/usr/local/bin:${PATH}"

COPY --from=builder /app/target/release/shipshape-server /usr/local/bin/shipshape-server

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/shipshape-server"]
