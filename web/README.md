# web/

Connectome-style frontend for the cortex-mechanism. Next.js 15 (app router) + React 19 + Tailwind v4.

See `vault/ideas/architecture-stack.md` for how this fits the larger system, and `vault/concepts/visualization.md` for design constraints.

## Run

```bash
cd web
npm install   # or pnpm install / bun install
npm run dev
```

Open http://localhost:3000.

## Layout

- `app/page.tsx` — three-pane dashboard (chat | connectome | drives)
- `components/ConnectomeView.tsx` — canvas-based graph, currently placeholder dynamics
- `components/ChatPanel.tsx` — text input; mic/image/cam are stubs
- `components/DriveDashboard.tsx` — free energy + intrinsic drives readout
- `components/StatusBar.tsx` — core connection state

## Next

The frontend currently fakes spike dynamics client-side. M1 from the architecture plan: subscribe to `ws://localhost:8080/spikes` from the Rust core and render real `SpikeFrame` events.
