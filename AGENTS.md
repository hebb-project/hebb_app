# Agent Operating Notes

This repo is a multi-agent research workspace. Assume other agents may be working in other worktrees or branches at the same time.

## Coordination

- Work in an isolated branch or worktree for non-trivial edits.
- Never add code or vault changes directly on `main`; create a branch/worktree first, even for planning notes or research logs.
- Do not overwrite or revert changes you did not make.
- Check `git status --short --branch` before editing and before handing off.
- Make small, logically scoped commits when a unit of work is complete.

## Project Memory

The Obsidian vault at `vault/` is the shared project memory and cross-agent coordination layer. Treat it as retrieval context before making claims about project intent, architecture, decisions, research status, or prior work.

- Start by reading `CLAUDE.md`, `vault/index.md`, and `vault/meta/conventions.md` when initializing into the repo.
- For research, architecture, planning, or "what did we decide" questions, search `vault/` first with `rg`, then cite relevant notes as `[[note-name]]`.
- After learning anything durable, append it to `vault/logs/YYYY-MM-DD.md` and create or update topic notes when useful.
- Link new notes from the relevant folder `index.md`; use Obsidian `[[wiki-links]]` liberally.
- Prefer adding concise, retrievable notes over burying decisions only in chat.

## Current Aim

The repo is building a "cortex mechanism": a persistent, event-driven, compute-efficient agent substrate with intrinsic goals, online learning, and long-horizon planning. LLMs, VLMs, CV, ASR, and similar models are expected to be tool modules, not the core substrate.

Near-term implementation centers on:

- `core/`: Rust SNN/event-driven substrate and server.
- `web/`: Next.js visualization and interaction surface.
- `experiments/`: Python research/prototyping layer.
- `vault/`: Obsidian knowledge base and shared memory.
