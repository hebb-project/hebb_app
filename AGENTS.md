# Agent Operating Notes

This repo is the Hebb desktop app and visualizer. Assume other agents may be working in other worktrees or branches at the same time.

## Coordination

- Work in an isolated branch or worktree for non-trivial edits.
- Never add code directly on `main`; create a branch first.
- Do not overwrite or revert changes you did not make.
- Check `git status --short --branch` before editing and before handing off.
- Make small, logically scoped commits when a unit of work is complete.

## Current Aim

A local-first desktop app and visualizer for spiking-neural-network cortexes. Researchers and learners can create biological-neuron networks, run them, persist them to disk, and inspect spiking activity in a connectome viewer.

## Repo layout

- `core/` — Rust axum server hosting the simulator engine actor, REST + WebSocket API, Postgres ORM via Diesel.
- `web/` — Next.js + Tailwind frontend. Connectome viewer, neural inspector.
- `desktop/` — Tauri 2 shell bundling `core` + `web` with an embedded Postgres supervisor.
- `tests/` — Integration fixtures.

## Where things should live

- **SNN substrate changes** (new neuron model, plasticity rule, on-disk format) → not here. Belongs in [hebb-project/hebb](https://github.com/hebb-project/hebb). After landing there and publishing a new `hebb` version on crates.io, bump the dep in `core/Cargo.toml` and `desktop/src-tauri/Cargo.toml`.
- **API / persistence / agent loop** → `core/`.
- **UI / IPC commands** → `web/` and `desktop/src-tauri/`.
