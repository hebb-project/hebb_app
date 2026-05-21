# Contributing

This project is a local-first computational-neuroscience workspace. The core
product goal is simple: a researcher or learner can create a biological-neuron
network, run it, visualize it, persist it on disk, and open that same network
from Rust, Python, or the desktop UI.

The repo is intentionally multi-surface:

- Rust owns the substrate and server.
- Python owns research/prototyping ergonomics.
- The desktop and web UI own visualization and interaction.
- The Obsidian vault owns project memory and design rationale.

Read `CLAUDE.md` and `vault/ideas/index.md` before making architecture claims.

## Setup

Install these tools first:

- Rust stable toolchain with `cargo`, `rustfmt`, and `clippy`.
- Node.js and npm for the web and desktop shells.
- Python 3.11+.
- `uv` for Python environment management, or `pip` as a fallback.
- `go-task` (`task`) for repo task shortcuts.
- Docker if you want the local Postgres-backed stack.

Install project dependencies:

```sh
task install
```

Build and install the Python binding into the active environment:

```sh
task py:snn
```

Run the local stack:

```sh
task dev
```

In another terminal, run either the web UI or the desktop shell:

```sh
task web:dev
task desktop:dev
```

Useful setup notes:

- `task install` delegates to `web:install` and `py:install`.
- `task py:snn` uses `maturin` through `uv` and the `cortex-py/Cargo.toml`
  manifest.
- `task dev` runs Docker Compose for the server/Postgres stack.
- `task desktop:dev` boots the Tauri shell and its webview.

## Architecture Overview

`cortex-snn` is the simulator substrate. It owns neuron traits, synapse traits,
the event-driven simulation engine, and the `.cortex/` disk-format codecs. It
must stay usable as a library. Pure serde/byte codecs are always available;
filesystem I/O belongs behind optional features so embedding consumers do not
pay for disk behavior they did not request.

`core` is the Rust server. It owns HTTP/WebSocket APIs, Postgres integration,
engine hydration, spike/weight persistence loops, and the bridge between UI
events and simulation state. Postgres is allowed to index or cache derived
information, but LIF/HH topology and weights should come from `.cortex/` files
as the disk-format chain lands.

`cortex-py` is the PyO3 wrapper over `cortex-snn`. Its job is to make the same
network folder accessible to Python researchers without requiring an HTTP
server. Python APIs should mirror the Rust handle where possible and provide
numpy-friendly hot paths for large weight arrays.

`desktop` is the Tauri shell. It owns native folder picking, process
supervision, embedded/external Postgres selection, and desktop packaging. It
should not duplicate simulator logic; it should launch services and persist
desktop metadata through the shared folder format.

`web` is the Next.js visualization and interaction surface. It owns the
connectome view, chat/search panels, start screen, and WebSocket rendering of
spikes/weights. It should treat the server as the source of runtime truth and
avoid reimplementing simulation semantics client-side.

`experiments` is the Python research layer. It contains scripts, benchmark
tools, and exploratory prototypes. Keep research code useful and reproducible,
but do not make product-critical behavior exist only in experiments.

`vault` is the shared project memory. Durable design decisions, experiment
results, open questions, and coordination notes should be written there using
Obsidian wiki links. If you learned something that future contributors need, do
not bury it only in a PR comment.

## How To Add A New Neuron Kind

The current worked examples are `LifNeuron` and `HhNeuron` in
`cortex-snn/src/domain/neuron.rs`. The trait to implement is `Neuron`.

### 1. Implement The `Neuron` Trait

A neuron implementation must provide:

- `node_id()` returning the stable node UUID.
- `tick(input_current, ctx)` advancing state by `ctx.dt_ms` and returning
  whether the neuron fired.
- `membrane_potential()` for visualization and diagnostics.
- `reset()` for returning to a default state.
- `serialize_state()` for `state/{type}/latest.json` persistence.

Follow the reference patterns:

- `LifNeuron` is a simple reset-on-threshold model with refractory handling and
  an exponentially decaying post-synaptic current.
- `HhNeuron` is a Hodgkin-Huxley ODE model with Euler/RK4 integrators, gating
  variables, threshold-crossing spike detection, and no artificial voltage
  reset.

Keep units explicit in comments and field names. New neuron models should reject
or clamp nonsensical internal state at construction boundaries, not deep inside
the engine loop.

### 2. Register A Factory In The Engine

The engine needs a place to turn a disk/wire kind into a concrete neuron. When
the disk-format handle hydrates a topology row, it resolves:

```json
{ "kind": "hh", "config": { "integrator": "rk4" } }
```

into the corresponding Rust constructor.

Add a factory entry for the new kind wherever the current engine hydration path
maps `NeuronSpec.kind` to `LifNeuron`, `HhNeuron`, or future implementations.
Do not silently fallback to LIF for unknown kinds. Unknown kinds should produce
a clear error that includes the unsupported slug.

### 3. Add The Kind Slug To Docs And Tests

Neuron kind strings are kebab-case slugs. Use examples like:

- `izhikevich`
- `adex`
- `hh-2c`
- `rate-coded`

The disk format does not need a version bump for a new kind if the existing
`{ "kind": "...", "config": ... }` envelope can represent it. Update:

- Schema docs in `cortex-snn/SCHEMA.md`.
- Any constructor/factory tests.
- Example fixtures if the new kind should be contributor-facing.
- UI allowlists or labels if users can create the kind from the desktop/web.

## How To Add A New Synapse Kind

The current worked example is `StdpSynapse` in
`cortex-snn/src/domain/synapse.rs`. Implement `Synapse` with:

- Stable `id`, `pre_id`, and `post_id`.
- `weight()` and `set_weight()` with appropriate clamping.
- `transmit(pre_fired)` for the current contribution.
- `update(ctx)` for learning and trace evolution.
- `serialize_state()` for future persistence.

Use a kebab-case `SynapseSpec.kind`, for example `stdp` or
`plastic-synapse`. As with neurons, adding a new kind usually does not require a
format bump.

## How To Bump A Format Version

Format bumps should be rare. Prefer additive fields with serde defaults when
possible.

Use the v1-to-v2 metadata migration in
`cortex-snn/src/format/metadata.rs::MetadataFile::from_json_bytes` as the
canonical pattern:

1. Deserialize into a shape that can read both old and current fields.
2. Check the file version before normal validation.
3. Map legacy values into the current in-memory representation.
4. Reject unknown legacy values rather than guessing.
5. Set the in-memory version to the current version.
6. Clear deprecated fields that should not be emitted by current writers.
7. Run current validation on the migrated value.
8. Return the migrated value without rewriting disk unless the caller asked for
   an upgrade write.

Every bump needs tests for:

- Current-version round trip.
- Supported old-version migration.
- Unknown future-version rejection.
- Unknown legacy enum/string rejection.
- Validation of any new invariants.

For binary formats, bump the version when record layout changes. A v1 reader
must reject an unexpected `record_size`; it must not guess how to skip fields.

## Testing

Run the smallest relevant checks before committing:

```sh
task core:test
cargo test -p cortex-snn --features disk
task py:snn-smoke
task web:typecheck
```

Use focused tests first, then widen:

- For simulator changes, run the relevant `cargo test -p cortex-snn ...` target.
- For server changes, run `task core:test`.
- For PyO3 changes, run `task py:snn` and `task py:snn-smoke`.
- For frontend changes, run `task web:typecheck` and `task web:build`.
- For desktop supervision changes, run `task desktop:dev` when the local
  environment supports it.

Do not claim a build is clean unless you ran it in the branch you are handing
off.

## PR Conventions

Use small, logically scoped PRs. One PR should have one reason to exist.

Commit messages should explain why the change exists, not just what files moved.
Prefer:

```text
docs: specify cortex folder schema
```

over:

```text
update docs
```

For stacked work:

- Branch from the current intended base.
- Keep each PR reviewable on its own.
- Include the verification commands you ran.
- Mention any deliberately skipped checks.
- Do not mix generated churn with semantic changes.

Do not overwrite or revert changes you did not make. This repo is often used as
a multi-agent workspace; other branches and worktrees may be active at the same
time.

## Project Memory

Use the vault as shared memory:

- `CLAUDE.md` has repo operating instructions.
- `vault/index.md` is the vault entry point.
- `vault/meta/conventions.md` defines note/link conventions.
- `vault/ideas/index.md` links active design notes.
- `vault/logs/YYYY-MM-DD.md` captures session-level progress.

For research, architecture, or "what did we decide?" questions, search the
vault first and cite relevant notes with `[[wiki-links]]`. After learning
anything durable, append it to the current daily log and update topic notes when
useful.
