"use client";

import dynamic from "next/dynamic";
import { useEffect, useState } from "react";
import { ChatPanel } from "@/components/ChatPanel";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { Header } from "@/components/Header";
import { StartScreen, type NetworkRecord } from "@/components/StartScreen";
import { VaultSearchPanel } from "@/components/VaultSearchPanel";
import { getCortexFolder } from "@/lib/cortex-api";
import { onDesktopMenuAction } from "@/lib/desktop";
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
const SEARCH_WIDTH = 360;
const NODE_COUNT = 140;
// Enable live mode by default; flip off with NEXT_PUBLIC_CORTEX_LIVE=0.
const LIVE = process.env.NEXT_PUBLIC_CORTEX_LIVE !== "0";

export default function Home() {
  const [network, setNetwork] = useState<NetworkRecord | null>(null);
  const [stateKey, setStateKey] = useState<StateKey>("idle");
  const [spikeRate, setSpikeRate] = useState(0);
  const [uptime, setUptime] = useState(347);
  const [activeFolder, setActiveFolder] = useState<string | null>(null);

  useEffect(() => {
    if (stateKey === "offline") return;
    const id = setInterval(() => setUptime((u) => u + 1), 1000);
    return () => clearInterval(id);
  }, [stateKey]);

  useEffect(() => {
    let dispose = () => {};
    onDesktopMenuAction((action) => {
      if (action === "new-window" || action === "open-network") {
        setNetwork(null);
        setStateKey("idle");
        setSpikeRate(0);
        setUptime(0);
        setActiveFolder(null);
      }
    }).then((unlisten) => {
      dispose = unlisten;
    });

    return () => dispose();
  }, []);

  useEffect(() => {
    if (!network) {
      setActiveFolder(null);
      return;
    }

    let cancelled = false;
    getCortexFolder()
      .then((status) => {
        if (!cancelled) setActiveFolder(status.folder);
      })
      .catch(() => {
        if (!cancelled) setActiveFolder(null);
      });
    return () => {
      cancelled = true;
    };
  }, [network]);

  if (!network) {
    return (
      <ErrorBoundary>
        <StartScreen onOpen={setNetwork} />
      </ErrorBoundary>
    );
  }

  return (
    <ErrorBoundary>
      <div className="app">
        <Header
          stateKey={stateKey}
          spikeRate={spikeRate}
          uptime={uptime}
          networkName={network.name}
          activeFolder={activeFolder}
          onOpenStart={() => setNetwork(null)}
        />
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
              key={network.id}
              stateKey={stateKey}
              nodeCount={NODE_COUNT}
              palette={DEFAULT_PALETTE}
              onSpikeRate={setSpikeRate}
              live={LIVE}
              onReopen={() => setNetwork(null)}
            />
          </ErrorBoundary>
          <ErrorBoundary>
            <VaultSearchPanel
              width={SEARCH_WIDTH}
              live={LIVE}
              onStimulate={() => {
                if (stateKey === "idle") setStateKey("active");
              }}
            />
          </ErrorBoundary>
        </div>
      </div>
    </ErrorBoundary>
  );
}
