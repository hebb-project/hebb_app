# syntax=docker/dockerfile:1.7

# ─── builder ──────────────────────────────────────────────────────────────
# Use current stable rust. The dep tree (envy → toml → serde_spanned 1.1.1)
# requires edition2024 which only stabilizes in 1.85+.
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# ── dep-cache prime ──
# Copy the workspace manifest + each member's manifest, then build with
# empty stub sources. This populates `/build/target` with the entire
# compiled dep graph so a real source change doesn't re-pull/re-compile
# crates.io packages. The workspace root must contain the same set of
# members the host has (core + cortex-snn); `desktop/src-tauri` and
# `cortex-py` are excluded by `Cargo.toml`'s workspace.exclude list and
# don't need to ship in the build context.
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml ./core/
COPY cortex-snn/Cargo.toml ./cortex-snn/

RUN mkdir -p core/src cortex-snn/src \
    && echo "fn main() {}" > core/src/main.rs \
    && : > cortex-snn/src/lib.rs \
    && cargo build --release --locked --package core --bin core \
    && rm -rf core/src cortex-snn/src \
        target/release/deps/core-* \
        target/release/deps/cortex_snn-* \
        target/release/deps/libcortex_snn-* \
        target/release/.fingerprint/core-* \
        target/release/.fingerprint/cortex-snn-* \
        target/release/core* \
        target/release/libcortex_snn*

# ── real sources ──
COPY core/src ./core/src
COPY core/migrations ./core/migrations
COPY core/diesel.toml ./core/diesel.toml
COPY cortex-snn/src ./cortex-snn/src

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

# Bake the mock knowledge base in at /vault as the default ingest source.
# Users override with a bind mount to their own vault in docker-compose
# (production usage). Baking it in avoids macOS Docker Desktop's
# virtiofs/grpcfuse EDEADLK bug on bind-mounted file reads for the
# out-of-the-box demo.
COPY --chown=cortex:cortex tests/mock-knowledge-base /vault

USER cortex
EXPOSE 8080
ENV RUST_LOG=info
CMD ["cortex-core"]
