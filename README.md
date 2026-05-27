<p align="center">
  <img src="desktop/src-tauri/icons/icon.png" alt="Hebb app icon" width="160" />
</p>

<h1 align="center">Hebb App</h1>

<p align="center">
  <em>An event-driven, continually-learning AI substrate with emergent goals — a brain, not a bigger LLM.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-Apache%202.0-yellow.svg" alt="License: Apache 2.0" />
  <img src="https://img.shields.io/badge/status-alpha-orange.svg" alt="Status: alpha" />
  <img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs welcome" />
</p>

Hebb is the product-facing shell for the broader **Long Horizon Cortex Mechanism** research project: a persistent, event-driven substrate for long-horizon planning, online learning, and tool-using intelligence — one where traditional neural networks (Transformers, ResNets, encoders) are *modules*, not the core architecture.

## What is this?

Hebb is two things at once, and you can use whichever you need:

1. **A spiking-neural-network simulator and brain-inspired memory substrate you can use today.** In neuroscience, the *cortex* is the part of the brain that learns continuously from a stream of events, stores what it learns as a graph of connections, and spends energy only where something is actually firing. Hebb brings that model to software: a fast Rust spiking-network core, Python bindings, a live connectome viewer, and a knowledge-graph memory layer. If you do computational neuroscience, neuromorphic, or SNN research, this is a scriptable, visualizable spiking substrate you can drop into your current work.

2. **A long-term bet on a different kind of AI.** The same substrate is the seed of an agentic system with intrinsic goals that evolves online and runs on sparse, event-driven compute — see [Vision](#vision).

> Don't know what a "cortex" means here? Read it as **brain-inspired memory + compute substrate**. The simplest useful form is just a knowledge graph that *learns* — which is why the early product is approachable even if you never touch a spiking neuron.

## Who is this for?

| You are… | Use Hebb as… | Start here |
| --- | --- | --- |
| A **computational-neuroscience / SNN / neuromorphic researcher** | A fast, scriptable, visualizable spiking-network simulator (Rust core + `hebb` Python bindings) | [Use as a library](#use-as-a-library) |
| A **knowledge / memory tooling builder** (Obsidian power-users now, non-technical users later) | A graph-native "neural memory" over a single-folder vault — retrieval as spike cascades through a learned graph, not vector RAG | [Vision](#vision) |
| An **AI-architecture / AGI researcher** | A research platform for event-driven, continually-learning, goal-driven agents | [Vision](#vision) |

## Vision

LLMs are excellent interpolators over recorded human output, but they don't *want* anything, don't *evolve* between sessions, and pay for every token in dense matmul. We're building toward the opposite: an **agentic** system with intrinsic goals, that **evolves** continually online, and runs on **sparse, event-driven** compute — closer to a cortex than a transformer.

Anchoring threads:

- **Spiking neural networks** as the algorithmic substrate, with neuromorphic hardware on the horizon.
- **Active inference** as a principled origin of goals via preferred-state distributions.
- **Continual learning** so the system evolves without catastrophic forgetting.
- **Knowledge-graph-native memory** — retrieval as spike cascades through a learned graph, not vector RAG.
- **Converging cortexes.** Different cortex *topologies* and on-disk *formats* (knowledge-graph, LIF, Hodgkin–Huxley, …) are converging on a shared interchange format so a network built in one surface opens in another — and cortexes can query, stimulate, and eventually intermingle with each other. The simplest topology (a knowledge graph that learns) is a usable tool on its own; the richer ones grow out of it.

The bigger picture is swarms of evolutionary, bio-inspired neural networks interoperating with traditional systems and knowledge graphs.

## Repository layout

- `core/` — Rust axum/tokio server. Hosts the simulator engine actor, REST + WebSocket API, Postgres ORM via Diesel.
- `web/` — Next.js + Tailwind frontend: connectome viewer, start screen, neural inspector.
- `desktop/` — Tauri 2 shell that bundles `core` + `web` into a local-first app with an embedded Postgres supervisor.
- `tests/` — integration fixtures (mock + classical knowledge bases).
- `scripts/` — development utilities.

The SNN simulator itself lives in a separate repo: [hebb-project/hebb](https://github.com/hebb-project/hebb), published as the `hebb` crate on [crates.io](https://crates.io/crates/hebb) and `hebb-py` on PyPI. `core/` and `desktop/src-tauri/` consume it as a versioned dependency.

## Phased roadmap

Short version:

1. **Now** — visualize a spiking network; show learned skills and connections forming. Local-first Hebb app that opens or creates a single-folder workspace, with durable metadata, weights, and runtime state attached to that folder.
2. **Knowledge-graph init / absorption** — bootstrap a cortex system from an existing vault.
3. **Multimodal tool integration** — route to CV / RL / LLM / VLM / ASR / TTS modules per task.
4. **Learning + motivation** — keep-alive evolution, intrinsic drives.
5. **Embodiment** — robotic control.
6. **Swarm / Von Neumann probes** — long-horizon collaborative agents. Speculative end-state.

## Demos

> Screen recordings of the connectome viewer (live spike propagation, STDP weights forming, the neural inspector) are coming soon.

<!-- TODO: drop demo gifs/screenshots here, e.g. docs/demos/connectome.gif -->

## Getting started

### Desktop app

The local-first desktop app is the easiest way to *see* a cortex run. Prebuilt installers are on the way:

| Platform | Status |
| --- | --- |
| macOS (Apple Silicon / Intel) | Coming soon |
| Windows | Coming soon |
| Linux (AppImage / `.deb`) | Coming soon |

Until installers ship, run the desktop shell from source — see [Local development](#local-development).

### Use as a library

If you only want the SNN substrate (not the app), install from the standalone library repo:

```bash
# Rust
cargo add hebb

# Python
pip install hebb-py
```

```python
import hebb

sim = hebb.Sim()
a = sim.add_neuron()
b = sim.add_neuron()
sim.add_edge(a, b, weight=0.9)
sim.stimulate(a, current=50.0, duration_ms=30.0)

spikes = sim.run(dt_ms=1.0, n_steps=200)   # -> [(neuron_id, t_ms), ...]
```

The library is in [hebb-project/hebb](https://github.com/hebb-project/hebb).

### Local development

**Prerequisites:**

- [Task](https://taskfile.dev) (`go-task`) — the task runner used below
- Docker (for the Postgres dev database)
- A Rust toolchain (`cargo`) — builds the `core/` server
- Node.js ≥ 20.19 — runs the `web/` frontend
- For `desktop:dev` only: the [Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/)

**First-time setup:**

```bash
cp .env.example .env   # sets DATABASE_URL for the core (required)
```

**Full dev stack** — Postgres (Docker) + Rust core + web dev server on the host:

```bash
task dev
```

> The first run compiles the Rust core in release mode (~1–2 min). Once up:
> web frontend on http://localhost:3737, core API/WebSocket on http://localhost:7654.
> Only Postgres runs in Docker; the core and web server run on your host.

**Desktop app** — Tauri shell that boots the web dev server and opens a webview window:

```bash
task desktop:dev
```

**Browser-only frontend** (no core):

```bash
cd web
npm install
npm run dev
```

`task` (no args) lists every available command. `task dev:down` stops the Postgres container.

## Future directions

The project deliberately keeps doors open. Active directions:

- **Knowledge-graph memory** — Hebbian retrieval over an Obsidian-style vault, no vector DB.
- **Cortex-as-a-service** — expose a cortex over MCP so other agents (Claude, etc.) can think *through* it.
- **Swarm / collaborative cortexes** — per-agent and per-user cortexes that query and intermingle.
- **Explore↔exploit knob** — active-inference trade-off, both agent-learned and user-overridable.
- **Natural evolution under budgets** — adaptive sparsity, pruning, and compaction under bounded compute/memory.
- **Converging topologies & formats** — one interchange format across knowledge-graph / LIF / Hodgkin–Huxley cortexes.

## Contributing & conventions

- `CONTRIBUTING.md` — workflow basics.
- `AGENTS.md` — guidance for AI coding agents working in this repo.

## License

Hebb has an Apache 2.0 license, as found in the [LICENSE](LICENSE) file.
