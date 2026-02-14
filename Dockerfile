# syntax=docker/dockerfile:1.7

FROM rust:1.76-alpine AS builder
WORKDIR /app

RUN apk add --no-cache \
    build-base \
    clang \
    git \
    llvm \
    openssl-dev \
    perl \
    pkgconfig \
    postgresql-dev

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

FROM alpine:3.19 AS runtime

RUN apk add --no-cache \
    ca-certificates \
    clang \
    git \
    go \
    python3 \
    py3-pip \
    postgresql-libs \
    libgcc \
    libstdc++

ARG SHIPSHAPE_PIP_LIBS="cdd-c type-correct lib2notebook2lib go-auto-err-handling"
RUN python3 -m pip install --no-cache-dir --upgrade pip \
    && python3 -m pip install --no-cache-dir ${SHIPSHAPE_PIP_LIBS}

COPY --from=builder /app/target/release/shipshape-server /usr/local/bin/shipshape-server

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/shipshape-server"]
