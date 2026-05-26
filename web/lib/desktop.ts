"use client";

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type TauriGlobal = Window & { __TAURI_INTERNALS__?: unknown };

export type DesktopMenuAction = "new-window" | "open-network";

export function isTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean((window as TauriGlobal).__TAURI_INTERNALS__);
}

export async function pickDirectory(title: string): Promise<string | null> {
  if (isTauriRuntime()) {
    const selected = await open({
      title,
      directory: true,
      multiple: false,
    });
    return typeof selected === "string" ? selected : null;
  }

  return window.prompt(`${title}\n\nEnter an absolute folder path:`)?.trim() || null;
}

// ── .cortex/ metadata folder ─────────────────────────────────────────────
//
// Mirrors `desktop/src-tauri/src/cortex_folder.rs`. Outside of the Tauri
// runtime these resolve to `null`/no-op so a plain `next dev` workflow
// still renders the start screen, just without folder inspection.

/**
 * Cortex-type slug — kebab-case, mirrors `core::CortexType::slug()` and
 * `desktop/src-tauri/src/cortex_folder.rs::is_known_cortex_type`. The
 * widened `string` allows forward-compat: older clients reading a
 * future cortex type degrade to a label rather than a parse error.
 */
export type CortexTypeSlug = "knowledge-graph" | "lif" | "hh";

/**
 * HH neuron parameters that the user can override per network. Sent
 * opaquely through `hh_config` into `.cortex/metadata.json` and into
 * `POST /api/cortex`. Field shape mirrors `hebb::HhConfig`;
 * everything is optional so the desktop only sends what was changed
 * from defaults.
 */
export type HhConfigOverrides = {
  integrator?: "euler" | "rk4";
};

export type CortexMetadata = {
  version: number;
  id: string;
  name: string;
  cortex_type: CortexTypeSlug | string;
  hh_config?: HhConfigOverrides | null;
  source_root: string;
  created_at: string;
  updated_at: string;
};

export type CortexFolderInfo = {
  root: string;
  cortex_path: string;
  has_cortex: boolean;
  metadata: CortexMetadata | null;
  /** Node count from `topology.json`, or `null` when the file is absent. */
  node_count: number | null;
  /** Edge count from `topology.json`, or `null` when the file is absent. */
  edge_count: number | null;
};

export async function inspectCortexFolder(path: string): Promise<CortexFolderInfo | null> {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<CortexFolderInfo>("inspect_cortex_folder", { path });
  } catch (err) {
    console.warn("inspect_cortex_folder failed", err);
    return null;
  }
}

export async function initCortexFolder(
  path: string,
  name: string,
  cortexType: CortexTypeSlug,
  hhConfig?: HhConfigOverrides | null,
): Promise<CortexFolderInfo | null> {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<CortexFolderInfo>("init_cortex_folder", {
      path,
      name,
      cortexType,
      // The Rust signature accepts `Option<serde_json::Value>` so an
      // unspecified config goes over as `null`, not omitted. Tauri's
      // arg-coercion needs the field present.
      hhConfig: hhConfig ?? null,
    });
  } catch (err) {
    console.warn("init_cortex_folder failed", err);
    return null;
  }
}

// ── Seed network generators ───────────────────────────────────────────
//
// Mirrors `desktop/src-tauri/src/cortex_seeds.rs`. The host-side command
// runs `hebb::seeds` against a `Cortex` opened on the freshly
// initialized folder, so the user lands in the visualizer with a real
// starter network rather than an empty graph.

export type SeedKind = "random" | "ring" | "small_world" | "layered";

export type SeedConfig = {
  /** `[low, high]` for uniform-sampled init weights. Both in [0,1]; low ≤ high. */
  weight_range?: [number, number];
  /** Synaptic delay in ms (uniform across the seed). */
  delay_ms?: number;
  /** PRNG seed — pinning it makes the network reproducible. */
  seed?: number;
};

export type SeedSpec =
  | { kind: "random"; n: number; p: number; config?: SeedConfig }
  | { kind: "ring"; n: number; k: number; config?: SeedConfig }
  | { kind: "small_world"; n: number; k: number; p_rewire: number; config?: SeedConfig }
  | { kind: "layered"; layers: number[]; config?: SeedConfig };

export type SeedSummary = {
  added_nodes: number;
  added_edges: number;
  cortex_type: CortexTypeSlug;
};

/**
 * Populate a freshly-initialized .cortex/ folder with a generated
 * network. Must be called *after* `initCortexFolder` (the metadata
 * file is the validation gate for cortex type) and *before* the
 * visualizer opens — the topology.json gets written atomically by
 * the Rust side.
 *
 * Returns `null` outside the Tauri runtime; rejects from the Rust
 * side bubble up as thrown errors so the caller can surface them in
 * the status line.
 */
export async function seedCortexFolder(
  path: string,
  spec: SeedSpec,
): Promise<SeedSummary | null> {
  if (!isTauriRuntime()) return null;
  return await invoke<SeedSummary>("seed_cortex_folder", { path, spec });
}

export async function onDesktopMenuAction(
  handler: (action: DesktopMenuAction) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => {};

  const unlisten = await listen<DesktopMenuAction>("cortex://file-menu", (event) => {
    handler(event.payload);
  });
  return unlisten;
}
