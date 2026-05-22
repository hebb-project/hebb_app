"use client";

import { STATE_PRESETS, uptimeStr, type StateKey } from "@/lib/state";

type Props = {
  stateKey: StateKey;
  spikeRate: number;
  uptime: number;
  networkName: string;
  activeFolder: string | null;
  onOpenStart: () => void;
  /**
   * When set, a "re-open" button is shown next to the folder readout.
   * Clicking it re-opens the current `.cortex/` folder from disk so the
   * user can force-refresh after external changes without going back to
   * the start screen.
   */
  onReopenFolder?: () => void;
  /** True while a re-open is in progress — disables the button. */
  reopenPending?: boolean;
};

function shortFolder(path: string | null): string {
  if (!path) return "transient";
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 2) return path;
  return `.../${parts.slice(-2).join("/")}`;
}

export function Header({
  stateKey,
  spikeRate,
  uptime,
  networkName,
  activeFolder,
  onOpenStart,
  onReopenFolder,
  reopenPending = false,
}: Props) {
  const preset = STATE_PRESETS[stateKey];
  const online = stateKey !== "offline";
  // Show the re-open button only for folder-backed networks (activeFolder
  // is set and not the "transient" placeholder core uses for non-folder
  // sessions).
  const isFolderBacked = Boolean(activeFolder) && activeFolder !== "transient";
  return (
    <header className="header">
      <div className="header-left">
        <span className={`pulse-dot ${online ? "on" : "off"}`} />
        <span className="mono brand">Connectome Visualizer</span>
        <span className="mono brand-version">· v0</span>
        <button className="header-link mono" type="button" onClick={onOpenStart}>
          {networkName}
        </button>
      </div>
      <div className="header-right mono">
        <span className="readout">
          <span className="readout-k">core</span>
          <span className={`readout-v ${online ? "v-on" : "v-off"}`}>{preset.label}</span>
        </span>
        <span className="sep">/</span>
        <span className="readout">
          <span className="readout-k">uptime</span>
          <span className="readout-v">{uptimeStr(uptime)}</span>
        </span>
        <span className="sep">/</span>
        <span className="readout">
          <span className="readout-k">spikes/s</span>
          <span className="readout-v tabnum">{String(spikeRate).padStart(3, " ")}</span>
        </span>
        <span className="sep">/</span>
        <span className="readout" title={activeFolder ?? "no .cortex folder open"}>
          <span className="readout-k">folder</span>
          <span className="readout-v">{shortFolder(activeFolder)}</span>
        </span>
        {isFolderBacked && onReopenFolder && (
          <>
            <span className="sep">/</span>
            <button
              className="header-link mono"
              type="button"
              disabled={reopenPending}
              title="Re-open this .cortex/ folder from disk to pick up external changes"
              onClick={onReopenFolder}
              style={{ opacity: reopenPending ? 0.5 : 1 }}
            >
              {reopenPending ? "re-opening…" : "re-open"}
            </button>
          </>
        )}
        <span className="sep">/</span>
        <span className="readout">
          <span className="readout-k">ws</span>
          <span className="readout-v">core/spikes</span>
        </span>
      </div>
    </header>
  );
}
