export type StateKey = "idle" | "active" | "processing" | "offline";

export type DriveSet = {
  curiosity: number;
  homeostasis: number;
  social: number;
  rest: number;
};

export type Preset = {
  label: string;
  intensity: number;
  drives: DriveSet;
  freeEnergy: number;
};

export const STATE_PRESETS: Record<StateKey, Preset> = {
  idle: {
    label: "online",
    intensity: 0.18,
    drives: { curiosity: 0.42, homeostasis: 0.71, social: 0.28, rest: 0.66 },
    freeEnergy: 0.412,
  },
  active: {
    label: "active",
    intensity: 0.65,
    drives: { curiosity: 0.78, homeostasis: 0.58, social: 0.44, rest: 0.31 },
    freeEnergy: 0.687,
  },
  processing: {
    label: "processing",
    intensity: 0.92,
    drives: { curiosity: 0.91, homeostasis: 0.49, social: 0.36, rest: 0.18 },
    freeEnergy: 0.823,
  },
  offline: {
    label: "offline",
    intensity: 0.0,
    drives: { curiosity: 0, homeostasis: 0, social: 0, rest: 0 },
    freeEnergy: 0,
  },
};

export type Palette = {
  cyan: string;
  pink: string;
  purple: string;
};

export const DEFAULT_PALETTE: Palette = {
  cyan: "#7df9ff",
  pink: "#ff5d8f",
  purple: "#b794f6",
};

export function fmt3(n: number): string {
  return n.toFixed(3);
}

export function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

export function uptimeStr(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  return `${pad2(h)}:${pad2(m)}:${pad2(s)}`;
}
