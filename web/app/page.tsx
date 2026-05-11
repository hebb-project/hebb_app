"use client";

import dynamic from "next/dynamic";
import { useEffect, useState } from "react";
import { ChatPanel } from "@/components/ChatPanel";
import { DriveDashboard } from "@/components/DriveDashboard";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { Header } from "@/components/Header";
import { DEFAULT_PALETTE, type StateKey } from "@/lib/state";

// ConnectomeView is canvas-only with two WS subscriptions and an rAF
// loop — nothing about it benefits from SSR, and rendering it on the
// server makes Next/Turbopack do far more work on cold compile than
// the dev experience deserves. Defer to client only.
const ConnectomeView = dynamic(
  () => import("@/components/ConnectomeView").then((m) => m.ConnectomeView),
  {
    ssr: false,
    loading: () => (
      <main className="panel connectome">
        <div className="connectome-canvas-wrap">
          <div
            style={{
              position: "absolute", inset: 0,
              display: "grid", placeItems: "center",
              color: "rgba(125,249,255,0.4)",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
              fontSize: 12,
            }}
          >
            connectome · booting…
          </div>
        </div>
      </main>
    ),
  },
);

const CHAT_WIDTH = 320;
const DRIVES_WIDTH = 360;
const NODE_COUNT = 140;
// Enable live mode by default; flip off with NEXT_PUBLIC_CORTEX_LIVE=0.
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
    <ErrorBoundary>
      <div className="app">
        <Header stateKey={stateKey} spikeRate={spikeRate} uptime={uptime} />
        <div className="cols">
          <ErrorBoundary>
            <ChatPanel
              width={CHAT_WIDTH}
              live={LIVE}
              onSend={() => {
                if (stateKey === "idle") setStateKey("active");
              }}
            />
          </ErrorBoundary>
          <ErrorBoundary>
            <ConnectomeView
              stateKey={stateKey}
              nodeCount={NODE_COUNT}
              palette={DEFAULT_PALETTE}
              onSpikeRate={setSpikeRate}
              live={LIVE}
            />
          </ErrorBoundary>
          <ErrorBoundary>
            <DriveDashboard
              width={DRIVES_WIDTH}
              stateKey={stateKey}
              palette={DEFAULT_PALETTE}
            />
          </ErrorBoundary>
        </div>
      </div>
    </ErrorBoundary>
  );
}
