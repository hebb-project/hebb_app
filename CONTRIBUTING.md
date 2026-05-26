# Contributing to Hebb

Thanks for your interest! This repo is the desktop app and visualizer for Hebb. The simulator substrate lives in a separate repo: [hebb-project/hebb_core](https://github.com/hebb-project/hebb_core) (published to crates.io as `hebb` and PyPI as `hebb-py`).

## Repository layout

- `core/` — Rust axum/tokio server (the simulator engine actor, REST + WebSocket API, Postgres ORM)
- `web/` — Next.js + Tailwind frontend (connectome viewer, neural inspector)
- `desktop/` — Tauri 2 shell that bundles `core` + `web` into a local-first desktop app
- `tests/` — Integration fixtures (mock + classical knowledge bases)
- `scripts/` — Development utilities

## Where things should live

- **New SNN feature (neuron model, synapse plasticity rule, on-disk format change):** belongs in [hebb-project/hebb_core](https://github.com/hebb-project/hebb_core), not here. After landing there and publishing a new `hebb` version on crates.io, bump the `hebb = "..."` line in `core/Cargo.toml` and `desktop/src-tauri/Cargo.toml`.
- **New API endpoint, persistence change, agent loop hook:** `core/` here.
- **New visualization, UI panel, IPC command:** `web/` (UI) and possibly `desktop/src-tauri/` (Tauri command surface).

## Getting started

```bash
# Install
task install

# Bring up a full dev stack (postgres + core + web dev server)
task dev

# Or, for the Tauri desktop shell:
task desktop:dev
```

## Tests

```bash
task core:test        # cargo test -p core
task web:typecheck    # tsc --noEmit
task web:lint         # next lint
task web:build        # validates the production build
```

## Style

- `cargo fmt` for Rust.
- The web app uses ESLint + TypeScript strict mode.
- `core/Cargo.toml` has `[lints.rust] warnings = "deny"` — fix warnings, don't `#[allow]` them.

## License

Apache 2.0.
