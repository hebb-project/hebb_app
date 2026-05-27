# Contributing to Hebb

Thanks for your interest! This repo is the desktop app and visualizer for Hebb. The simulator substrate lives in a separate repo: [hebb-project/hebb](https://github.com/hebb-project/hebb) (published to crates.io as `hebb` and PyPI as `hebb-py`).

## Repository layout

- `core/` — Rust axum/tokio server (the simulator engine actor, REST + WebSocket API, Postgres ORM)
- `web/` — Next.js + Tailwind frontend (connectome viewer, neural inspector)
- `desktop/` — Tauri 2 shell that bundles `core` + `web` into a local-first desktop app
- `tests/` — Integration fixtures (mock + classical knowledge bases)
- `scripts/` — Development utilities

## Where things should live

- **New SNN feature (neuron model, synapse plasticity rule, on-disk format change):** belongs in [hebb-project/hebb](https://github.com/hebb-project/hebb), not here. After landing there and publishing a new `hebb` version on crates.io, bump the `hebb = "..."` line in `core/Cargo.toml` and `desktop/src-tauri/Cargo.toml`.
- **New API endpoint, persistence change, agent loop hook:** `core/` here.
- **New visualization, UI panel, IPC command:** `web/` (UI) and possibly `desktop/src-tauri/` (Tauri command surface).

## Getting started

Prerequisites: [Task](https://taskfile.dev) (`go-task`), a Rust toolchain (`cargo`), and Node.js ≥ 20.19. `task dev` (the desktop shell) also needs the [Tauri 2 system deps](https://v2.tauri.app/start/prerequisites/); `task dev:web` needs Docker (for Postgres).

```bash
# Install web + desktop deps
task install

# Run the desktop app: Tauri shell with embedded Postgres + core + a native window.
task dev

# Or develop in the browser: Postgres (Docker) + core + Next.js dev server.
# First run compiles the Rust core in release mode (~1-2 min).
# web -> http://localhost:3737, core -> http://localhost:7654
task dev:web

# The core defaults to the dev:web Postgres (host port 5433); only override it
# (a non-default DB or ports) via:  cp .env.example .env
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
