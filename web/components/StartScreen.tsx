"use client";

import { useEffect, useMemo, useState } from "react";
import { ingestVault } from "@/lib/cortex-api";
import {
  initCortexFolder,
  inspectCortexFolder,
  isTauriRuntime,
  pickDirectory,
} from "@/lib/desktop";

export type NetworkOrigin = "demo" | "knowledge-graph" | "fresh";

export type NetworkRecord = {
  id: string;
  name: string;
  origin: NetworkOrigin;
  folderPath: string;
  hasMetadata: boolean;
  createdAt: string;
  lastOpenedAt: string;
  nodeEstimate?: number;
  edgeEstimate?: number;
};

type Props = {
  onOpen: (network: NetworkRecord) => void;
};

const STORAGE_KEY = "cortex.networks.v1";

const DEMO_NETWORK: NetworkRecord = {
  id: "placeholder-network",
  name: "Placeholder Network",
  origin: "demo",
  folderPath: "local demo environment",
  hasMetadata: false,
  createdAt: "2026-05-14T00:00:00.000Z",
  lastOpenedAt: "2026-05-14T00:00:00.000Z",
  nodeEstimate: 140,
};

function readNetworks(): NetworkRecord[] {
  if (typeof window === "undefined") return [DEMO_NETWORK];

  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]");
    if (Array.isArray(parsed) && parsed.length > 0) {
      return parsed.map((item) => ({
        ...item,
        folderPath: item.folderPath ?? item.environmentPath ?? item.knowledgeGraphPath,
        hasMetadata: Boolean(item.hasMetadata),
      }));
    }
  } catch {
    // Fall through to the demo network; corrupt local shell state is non-fatal.
  }

  return [DEMO_NETWORK];
}

function writeNetworks(networks: NetworkRecord[]) {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(networks));
}

function shortPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 3) return path;
  return `.../${parts.slice(-3).join("/")}`;
}

function nameFromPath(path: string, fallback: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) || fallback;
}

export function StartScreen({ onOpen }: Props) {
  const [networks, setNetworks] = useState<NetworkRecord[]>([DEMO_NETWORK]);
  const [mode, setMode] = useState<"existing" | "knowledge-graph" | "fresh">("existing");
  const [folderPath, setFolderPath] = useState("");
  const [status, setStatus] = useState("Select the folder where this cortex lives.");
  const [pending, setPending] = useState(false);

  useEffect(() => {
    setNetworks(readNetworks());
  }, []);

  const canCreate = useMemo(() => {
    if (pending) return false;
    if (mode === "knowledge-graph" || mode === "fresh") return folderPath.trim();
    return false;
  }, [folderPath, mode, pending]);

  function persistAndOpen(network: NetworkRecord) {
    const updated = [
      { ...network, lastOpenedAt: new Date().toISOString() },
      ...networks.filter((item) => item.id !== network.id),
    ];
    setNetworks(updated);
    writeNetworks(updated);
    onOpen(updated[0]);
  }

  async function chooseFolder() {
    const selected = await pickDirectory("Choose cortex folder");
    if (selected) setFolderPath(selected);
  }

  async function openFolder() {
    const selected = await pickDirectory("Open network folder");
    if (!selected) return;

    const existing = networks.find((network) => network.folderPath === selected);
    if (existing) {
      // Re-inspect so the metadata flag reflects the on-disk truth, not
      // the cached value from when the user first added this folder.
      const info = await inspectCortexFolder(selected);
      if (info) {
        existing.hasMetadata = info.has_cortex;
      }
      persistAndOpen(existing);
      return;
    }

    const info = await inspectCortexFolder(selected);
    if (info?.has_cortex) {
      persistAndOpen({
        id: info.metadata?.id ?? `existing-${Date.now()}`,
        name: info.metadata?.name ?? `${nameFromPath(selected, "Existing")} Cortex`,
        origin: (info.metadata?.source_kind === "fresh" ? "fresh" : "knowledge-graph"),
        folderPath: selected,
        hasMetadata: true,
        createdAt: info.metadata?.created_at ?? new Date().toISOString(),
        lastOpenedAt: new Date().toISOString(),
      });
      return;
    }

    setFolderPath(selected);
    setMode("knowledge-graph");
    if (info) {
      setStatus("No .cortex/ metadata found. Initialize this folder to make it a cortex.");
    } else if (!isTauriRuntime()) {
      setStatus("Folder selected. Initialize from folder or start fresh.");
    } else {
      setStatus("Folder selected. Couldn't inspect — initialize to create .cortex/.");
    }
  }

  async function createNetwork() {
    if (!canCreate) return;

    setPending(true);
    const now = new Date().toISOString();
    const origin: NetworkOrigin = mode === "knowledge-graph" ? "knowledge-graph" : "fresh";
    const network: NetworkRecord = {
      id: `${origin}-${Date.now()}`,
      name:
        origin === "knowledge-graph"
          ? `${nameFromPath(folderPath, "Knowledge Graph")} Cortex`
          : `${nameFromPath(folderPath, "Untitled")} Cortex`,
      origin,
      folderPath: folderPath.trim(),
      hasMetadata: false,
      createdAt: now,
      lastOpenedAt: now,
    };

    try {
      if (origin === "knowledge-graph") {
        setStatus("Initializing neural network from folder contents...");
        const summary = await ingestVault(network.folderPath);
        network.nodeEstimate = summary.total_nodes;
        network.edgeEstimate = summary.total_edges;
        setStatus(`Created ${summary.total_nodes} nodes and ${summary.total_edges} edges.`);
      } else {
        setStatus("Initializing fresh cortex in selected folder...");
      }

      // Always write the .cortex/ folder so subsequent opens recognize
      // this directory as a Cortex network. For "fresh", this is the
      // entire initialization. For "knowledge-graph", it sits next to
      // the now-ingested SNN.
      const info = await initCortexFolder(network.folderPath, network.name, origin);
      if (info?.has_cortex) {
        network.hasMetadata = true;
        network.id = info.metadata?.id ?? network.id;
        setStatus(
          origin === "knowledge-graph"
            ? `Neural network initialized · ${network.nodeEstimate ?? 0} nodes · .cortex/ written`
            : "Fresh cortex initialized · .cortex/ written"
        );
      }

      persistAndOpen(network);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`Network shell created; ingest is waiting for core: ${message}`);
      persistAndOpen(network);
    } finally {
      setPending(false);
    }
  }

  return (
    <main className="start-screen">
      <section className="start-shell">
        <div className="start-heading">
          <div>
            <div className="start-kicker mono">Cortex</div>
            <h1>Select an environment</h1>
          </div>
          <div className="start-status mono">{status}</div>
        </div>

        <div className="start-layout">
          <section className="start-existing">
            <div className="start-section-hd mono">local environments</div>
            <button className="open-folder-btn" type="button" onClick={openFolder}>
              Open Network Folder...
            </button>
            <div className="network-list">
              {networks.map((network) => (
                <button
                  key={network.id}
                  className="network-card"
                  onClick={() => persistAndOpen(network)}
                >
                  <div className="network-card-top mono">
                    <span>{network.origin.replace("-", " ")}</span>
                    <span>{network.nodeEstimate ?? 0} nodes</span>
                  </div>
                  <div className="network-card-name">{network.name}</div>
                  <div className="network-card-path mono">{shortPath(network.folderPath)}</div>
                  <div className="network-card-path mono">
                    {network.hasMetadata ? "metadata detected" : "metadata pending"}
                  </div>
                </button>
              ))}
            </div>
          </section>

          <section className="start-create">
            <div className="start-tabs" role="tablist" aria-label="Network creation mode">
              <button
                className={mode === "knowledge-graph" ? "active" : ""}
                onClick={() => setMode("knowledge-graph")}
                type="button"
              >
                Initialize from folder
              </button>
              <button
                className={mode === "fresh" ? "active" : ""}
                onClick={() => setMode("fresh")}
                type="button"
              >
                Fresh
              </button>
            </div>

            <div className="start-form">
              {mode === "knowledge-graph" && (
                <label className="path-picker">
                  <span className="mono">cortex folder</span>
                  <div>
                    <input value={folderPath} onChange={(e) => setFolderPath(e.target.value)} />
                    <button type="button" onClick={chooseFolder}>Choose</button>
                  </div>
                </label>
              )}

              {mode === "fresh" && (
                <label className="path-picker">
                  <span className="mono">cortex folder</span>
                  <div>
                    <input value={folderPath} onChange={(e) => setFolderPath(e.target.value)} />
                    <button type="button" onClick={chooseFolder}>Choose</button>
                  </div>
                </label>
              )}

              <button className="start-primary" type="button" disabled={!canCreate} onClick={createNetwork}>
                {pending ? "Creating..." : "Create and Open"}
              </button>
            </div>
          </section>
        </div>
      </section>
    </main>
  );
}
