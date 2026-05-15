"use client";

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

export async function onDesktopMenuAction(
  handler: (action: DesktopMenuAction) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => {};

  const unlisten = await listen<DesktopMenuAction>("cortex://file-menu", (event) => {
    handler(event.payload);
  });
  return unlisten;
}
