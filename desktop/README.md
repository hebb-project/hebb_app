# desktop/

Tauri 2 shell for the Cortex desktop app.

The frontend lives in [`../web`](../web) and is loaded by the shell as a
static export (production) or by attaching to the Next.js dev server
(development). The Rust core in [`../core`](../core) is launched as a
managed subprocess in [COR-33](https://linear.app/cortex-mechanism/issue/COR-33);
today the shell only renders the existing UI.

## Layout

```
desktop/
├── package.json              ← hosts @tauri-apps/cli; npm scripts wrap `tauri ...`
└── src-tauri/
    ├── Cargo.toml            ← Rust crate, the "main process"
    ├── tauri.conf.json       ← window, build, security config
    ├── build.rs              ← tauri_build::build()
    ├── src/main.rs           ← thin entrypoint
    ├── src/lib.rs            ← Tauri app + (future) IPC commands
    └── capabilities/
        └── default.json      ← what the main window may do
```

See [ADR-001](../vault/ideas/adr-001-tauri-vs-electron.md) for the
shell-choice rationale.

## Dev

```bash
# from repo root
task desktop:dev          # → cd desktop && npm run dev → tauri dev
```

`tauri dev` starts the Next.js dev server (via `beforeDevCommand` in
`tauri.conf.json`) and opens a desktop window pointing at
`http://localhost:3737`.

Prerequisite: Rust + Node + the Tauri 2 platform deps for your OS
(WebKitGTK / libsoup / libappindicator on Linux; nothing extra on
macOS/Windows beyond Xcode CLT / WebView2).

## Build

```bash
task desktop:build        # produces unsigned dev bundle
```

Production bundling — icons, signing, auto-update — is deferred to
COR-53. Until then `bundle.active` is `false` in `tauri.conf.json` so
the dev workflow doesn't require an `icons/` directory.

## What's NOT here yet

- Subprocess management (COR-33).
- Keychain / first-run wizard (COR-34).
- Vault picker (COR-35).
- Icons + signing + updater (COR-53).
