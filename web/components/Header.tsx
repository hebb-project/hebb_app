"use client";

import { STATE_PRESETS, uptimeStr, type StateKey } from "@/lib/state";

type Props = {
  stateKey: StateKey;
  spikeRate: number;
  uptime: number;
  networkName: string;
  onOpenStart: () => void;
};

export function Header({ stateKey, spikeRate, uptime, networkName, onOpenStart }: Props) {
  const preset = STATE_PRESETS[stateKey];
  const online = stateKey !== "offline";
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
        <span className="readout">
          <span className="readout-k">ws</span>
          <span className="readout-v">core/spikes</span>
        </span>
      </div>
    </header>
  );
}
