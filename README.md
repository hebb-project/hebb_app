# Hebb

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

The Hebb desktop app and visualizer. A local-first tool for building, running, persisting, and visualizing spiking-neural-network cortexes.

The SNN substrate itself (neuron models, plasticity, on-disk format) lives in [hebb-project/hebb_core](https://github.com/hebb-project/hebb_core) and is consumed here via the `hebb` crate on crates.io.

## Layout

- `core/` — Rust [axum](https://github.com/tokio-rs/axum) server hosting the simulator engine actor. REST + WebSocket API, Postgres ORM via Diesel.
- `web/` — [Next.js 15](https://nextjs.org) + Tailwind frontend. Connectome viewer, neural inspector, live spike streaming.
- `desktop/` — [Tauri 2](https://tauri.app) shell. Bundles `core` + `web` into a single local-first app with an embedded Postgres supervisor.
- `tests/` — Integration fixtures (mock + classical knowledge bases).
- `scripts/` — Development utilities.

## Quick start

Prerequisites: Rust stable, Node 20+, Docker (for Postgres), [Task](https://taskfile.dev).

```bash
# Install web + desktop deps
task install

# Bring up the dev stack (docker postgres + core + web dev server)
task dev
# → http://localhost:3737
```

For the Tauri desktop shell (boots its own embedded Postgres):

```bash
task desktop:dev
```

## Building the desktop app

```bash
task desktop:build
# → desktop/src-tauri/target/release/bundle/
```

## Related repos

- [hebb-project/hebb_core](https://github.com/hebb-project/hebb_core) — the SNN substrate. Published as `hebb` on [crates.io](https://crates.io/crates/hebb) and `hebb-py` on PyPI.

## License

Apache 2.0
