# syntax=docker/dockerfile:1.7

# ─── builder ──────────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# ── dep-cache prime ──
# Copy the workspace manifest + core's manifest, then build with an empty
# stub source. This populates `/build/target` with the compiled dep graph
# (including the `hebb` crate from crates.io) so a real source change
# doesn't re-pull/re-compile crates.io packages.
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml ./core/

RUN mkdir -p core/src \
    && echo "fn main() {}" > core/src/main.rs \
    && cargo build --release --locked --package core --bin core \
    && rm -rf core/src \
        target/release/deps/core-* \
        target/release/.fingerprint/core-* \
        target/release/core*

# ── real sources ──
COPY core/src ./core/src
COPY core/migrations ./core/migrations
COPY core/diesel.toml ./core/diesel.toml

RUN cargo build --release --locked --package core --bin core

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
COPY --from=builder /build/core/migrations /app/migrations

COPY --chown=cortex:cortex tests/mock-knowledge-base /vault

USER cortex
EXPOSE 8080
ENV RUST_LOG=info
CMD ["cortex-core"]
