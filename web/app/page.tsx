"use client";

import { useEffect, useState } from "react";
import { ConnectomeView } from "@/components/ConnectomeView";
import { ChatPanel } from "@/components/ChatPanel";
import { DriveDashboard } from "@/components/DriveDashboard";
import { Header } from "@/components/Header";
import { DEFAULT_PALETTE, type StateKey } from "@/lib/state";

const CHAT_WIDTH = 320;
const DRIVES_WIDTH = 360;
const NODE_COUNT = 140;
// Enable live mode by default in dev; flip off (or set NEXT_PUBLIC_CORTEX_LIVE=0)
// when the Rust core isn't running, to fall back to the mock animator.
const LIVE = process.env.NEXT_PUBLIC_CORTEX_LIVE !== "0";

export default function Home() {
  const [stateKey, setStateKey] = useState<StateKey>("idle");
  const [spikeRate, setSpikeRate] = useState(0);
  const [uptime, setUptime] = useState(347);

  useEffect(() => {
    if (stateKey === "offline") return;
    const id = setInterval(() => setUptime((u) => u + 1), 1000);
    return () => clearInterval(id);
  }, [stateKey]);

  return (
    <div className="app">
      <Header stateKey={stateKey} spikeRate={spikeRate} uptime={uptime} />
      <div className="cols">
        <ChatPanel
          width={CHAT_WIDTH}
          live={LIVE}
          onSend={() => {
            if (stateKey === "idle") setStateKey("active");
          }}
        />
        <ConnectomeView
          stateKey={stateKey}
          nodeCount={NODE_COUNT}
          palette={DEFAULT_PALETTE}
          onSpikeRate={setSpikeRate}
          live={LIVE}
        />
        <DriveDashboard width={DRIVES_WIDTH} stateKey={stateKey} palette={DEFAULT_PALETTE} />
      </div>
    </div>
  );
}
