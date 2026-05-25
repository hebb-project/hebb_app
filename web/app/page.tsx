"use client";

import dynamic from "next/dynamic";
import {
  useCallback,
  useEffect,
  useState,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { ChatPanel } from "@/components/ChatPanel";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { Header } from "@/components/Header";
import { StartScreen, type NetworkRecord } from "@/components/StartScreen";
import { VaultSearchPanel } from "@/components/VaultSearchPanel";
import { getCortexFolder, openCortexFolder, type OpenCortexResponse } from "@/lib/cortex-api";
import { onDesktopMenuAction, type CortexTypeSlug } from "@/lib/desktop";
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
const SIDEBAR_MIN_WIDTH = 260;
const SIDEBAR_MAX_WIDTH = 620;
const SIDEBAR_KEYBOARD_STEP = 24;
const NODE_COUNT = 140;
// Enable live mode by default; flip off with NEXT_PUBLIC_CORTEX_LIVE=0.
const LIVE = process.env.NEXT_PUBLIC_CORTEX_LIVE !== "0";

export default function Home() {
  const [network, setNetwork] = useState<NetworkRecord | null>(null);
  const [stateKey, setStateKey] = useState<StateKey>("idle");
  const [spikeRate, setSpikeRate] = useState(0);
  const [uptime, setUptime] = useState(347);
  const [activeFolder, setActiveFolder] = useState<string | null>(null);
  const [reopenPending, setReopenPending] = useState(false);
  const [chatWidth, setChatWidth] = useState(CHAT_WIDTH);
  const [searchWidth, setSearchWidth] = useState(SEARCH_WIDTH);

  const clampSidebarWidth = useCallback((width: number) => (
    Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)))
  ), []);

  const resizeByKeyboard = useCallback((
    side: "left" | "right",
    event: KeyboardEvent<HTMLDivElement>,
  ) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const direction = event.key === "ArrowRight" ? 1 : -1;
    if (side === "left") {
      setChatWidth((width) => clampSidebarWidth(width + direction * SIDEBAR_KEYBOARD_STEP));
    } else {
      setSearchWidth((width) => clampSidebarWidth(width - direction * SIDEBAR_KEYBOARD_STEP));
    }
  }, [clampSidebarWidth]);

  const beginSidebarResize = useCallback((
    side: "left" | "right",
    event: ReactPointerEvent<HTMLDivElement>,
  ) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = side === "left" ? chatWidth : searchWidth;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    document.body.classList.add("is-resizing-sidebar");

    const updateWidth = (clientX: number) => {
      const delta = clientX - startX;
      const nextWidth = side === "left" ? startWidth + delta : startWidth - delta;
      if (side === "left") {
        setChatWidth(clampSidebarWidth(nextWidth));
      } else {
        setSearchWidth(clampSidebarWidth(nextWidth));
      }
    };

    const handlePointerMove = (moveEvent: globalThis.PointerEvent) => updateWidth(moveEvent.clientX);
    const handlePointerUp = () => {
      document.body.classList.remove("is-resizing-sidebar");
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerUp);
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerUp);
  }, [chatWidth, clampSidebarWidth, searchWidth]);

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

  /**
   * Re-open the current folder-backed network from disk. This calls
   * `POST /api/cortex/open` for the network's folder so core re-hydrates
   * topology + weights without the user navigating back to the start screen.
   * The ConnectomeView will re-fetch the graph on its own retry loop after
   * core completes the open.
   *
   * Only available for folder-backed (non-demo, non-KG) networks.
   */
  async function reopenFolder() {
    if (!network || network.origin === "demo" || network.cortexType === "knowledge-graph") return;
    if (reopenPending) return;
    setReopenPending(true);
    try {
      const opened: OpenCortexResponse = await openCortexFolder(network.folderPath);
      const openedType = opened.cortex_type as CortexTypeSlug | undefined;
      setNetwork((prev) =>
        prev
          ? {
              ...prev,
              name: opened.name || prev.name,
              cortexType: openedType ?? prev.cortexType,
              origin: openedType ?? prev.origin,
              nodeEstimate: opened.n_nodes,
              edgeEstimate: opened.n_edges,
              hasMetadata: true,
              lastOpenedAt: new Date().toISOString(),
            }
          : prev,
      );
      // Refresh activeFolder in case core changed it.
      const status = await getCortexFolder();
      setActiveFolder(status.folder);
    } catch (err) {
      console.warn("[page] reopenFolder failed", err);
    } finally {
      setReopenPending(false);
    }
  }

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
          onReopenFolder={
            network.origin !== "demo" && network.cortexType !== "knowledge-graph"
              ? () => { void reopenFolder(); }
              : undefined
          }
          reopenPending={reopenPending}
        />
        <div className="cols">
          <ErrorBoundary>
            <ChatPanel
              width={chatWidth}
              live={LIVE}
              onSend={() => {
                if (stateKey === "idle") setStateKey("active");
              }}
            />
          </ErrorBoundary>
          <div
            className="sidebar-resizer left-resizer"
            role="separator"
            aria-label="Resize chat sidebar"
            aria-orientation="vertical"
            aria-valuemin={SIDEBAR_MIN_WIDTH}
            aria-valuemax={SIDEBAR_MAX_WIDTH}
            aria-valuenow={chatWidth}
            tabIndex={0}
            onPointerDown={(event) => beginSidebarResize("left", event)}
            onKeyDown={(event) => resizeByKeyboard("left", event)}
          />
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
          <div
            className="sidebar-resizer right-resizer"
            role="separator"
            aria-label="Resize vault search sidebar"
            aria-orientation="vertical"
            aria-valuemin={SIDEBAR_MIN_WIDTH}
            aria-valuemax={SIDEBAR_MAX_WIDTH}
            aria-valuenow={searchWidth}
            tabIndex={0}
            onPointerDown={(event) => beginSidebarResize("right", event)}
            onKeyDown={(event) => resizeByKeyboard("right", event)}
          />
          <ErrorBoundary>
            <VaultSearchPanel
              width={searchWidth}
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
