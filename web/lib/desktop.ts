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

export type CortexMetadata = {
  version: number;
  id: string;
  name: string;
  source_kind: "knowledge-graph" | "fresh" | string;
  source_root: string;
  created_at: string;
  updated_at: string;
};

export type CortexFolderInfo = {
  root: string;
  cortex_path: string;
  has_cortex: boolean;
  metadata: CortexMetadata | null;
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
  sourceKind: "knowledge-graph" | "fresh",
): Promise<CortexFolderInfo | null> {
  if (!isTauriRuntime()) return null;
  try {
    return await invoke<CortexFolderInfo>("init_cortex_folder", {
      path,
      name,
      sourceKind,
    });
  } catch (err) {
    console.warn("init_cortex_folder failed", err);
    return null;
  }
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
