# syntax=docker/dockerfile:1.7

# ─── builder ──────────────────────────────────────────────────────────────
FROM rust:1.83-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies separately from source.
COPY core/Cargo.toml core/Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/core-* target/release/core*

COPY core/src ./src
COPY core/migrations ./migrations
COPY core/diesel.toml ./diesel.toml

RUN cargo build --release --bin core

# ─── runtime ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq5 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --create-home --home-dir /home/cortex cortex \
    && mkdir -p /data/models /vault \
    && chown -R cortex:cortex /data /home/cortex

WORKDIR /app
COPY --from=builder /build/target/release/core /usr/local/bin/cortex-core
COPY --from=builder /build/migrations /app/migrations

USER cortex
EXPOSE 8080
ENV RUST_LOG=info
CMD ["cortex-core"]
