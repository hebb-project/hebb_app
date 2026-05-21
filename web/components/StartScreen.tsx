"use client";

import { useEffect, useMemo, useState } from "react";
import {
  configureCortex,
  CortexTypeBody,
  ingestVault,
  openCortexFolder,
} from "@/lib/cortex-api";
import {
  CortexTypeSlug,
  HhConfigOverrides,
  initCortexFolder,
  inspectCortexFolder,
  isTauriRuntime,
  pickDirectory,
} from "@/lib/desktop";

/**
 * Demo networks predate any cortex_type field; the union keeps them
 * inhabitable without changing the storage schema for existing users.
 */
export type NetworkOrigin = "demo" | CortexTypeSlug;

export type NetworkRecord = {
  id: string;
  name: string;
  origin: NetworkOrigin;
  cortexType: CortexTypeSlug;
  hhConfig?: HhConfigOverrides | null;
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
  cortexType: "lif",
  folderPath: "local demo environment",
  hasMetadata: false,
  createdAt: "2026-05-14T00:00:00.000Z",
  lastOpenedAt: "2026-05-14T00:00:00.000Z",
  nodeEstimate: 140,
};

/**
 * Card metadata for the cortex-type selector. Order is intentional —
 * KG first because it's the existing primary flow, LIF second as the
 * cheap fresh option, HH last as the new biophysical pick.
 */
const CORTEX_TYPE_CARDS: ReadonlyArray<{
  slug: CortexTypeSlug;
  title: string;
  blurb: string;
}> = [
  {
    slug: "knowledge-graph",
    title: "Knowledge Graph",
    blurb:
      "Ingest an Obsidian-style folder. Notes become nodes; links become edges. Runs LIF neurons over the imported topology.",
  },
  {
    slug: "lif",
    title: "LIF Network",
    blurb:
      "Fresh spiking network using leaky integrate-and-fire neurons. Cheap to scale; the substrate shipped since M0.",
  },
  {
    slug: "hh",
    title: "Hodgkin-Huxley Network",
    blurb:
      "Biophysical neurons with ion-channel dynamics. Richer spike patterns and adaptation; higher per-tick cost.",
  },
];

function readNetworks(): NetworkRecord[] {
  if (typeof window === "undefined") return [DEMO_NETWORK];

  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]");
    if (Array.isArray(parsed) && parsed.length > 0) {
      return parsed.map((item) => {
        // Migrate legacy records: `origin: "fresh"` is the M0 name for
        // what's now `lif`; KG/demo carry over unchanged. We also derive
        // `cortexType` for older records that don't have it, so the
        // viz layer never has to special-case "no cortex type set".
        const legacyOrigin = item.origin;
        const origin: NetworkOrigin =
          legacyOrigin === "fresh" ? "lif" : legacyOrigin;
        const cortexType: CortexTypeSlug =
          item.cortexType ??
          (origin === "demo" ? "lif" : (origin as CortexTypeSlug));
        return {
          ...item,
          origin,
          cortexType,
          hhConfig: item.hhConfig ?? null,
          folderPath: item.folderPath ?? item.environmentPath ?? item.knowledgeGraphPath,
          hasMetadata: Boolean(item.hasMetadata),
        };
      });
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

function cortexTypeBody(
  cortexType: CortexTypeSlug,
  hhConfig?: HhConfigOverrides | null,
): CortexTypeBody {
  if (cortexType === "hh") {
    return hhConfig ? { kind: "hh", config: hhConfig } : { kind: "hh" };
  }
  return { kind: cortexType };
}

/**
 * Tell the running core to switch its engine to `cortexType`. Failure
 * is logged but non-fatal — the network record still persists locally
 * so the user can retry once core is running.
 */
async function pushCortexTypeToCore(
  cortexType: CortexTypeSlug,
  hhConfig?: HhConfigOverrides | null,
): Promise<boolean> {
  try {
    await configureCortex(cortexTypeBody(cortexType, hhConfig));
    return true;
  } catch (err) {
    console.warn("configureCortex failed", err);
    return false;
  }
}

export function StartScreen({ onOpen }: Props) {
  const [networks, setNetworks] = useState<NetworkRecord[]>([DEMO_NETWORK]);
  const [cortexType, setCortexType] = useState<CortexTypeSlug>("knowledge-graph");
  const [hhIntegrator, setHhIntegrator] = useState<"euler" | "rk4">("euler");
  const [folderPath, setFolderPath] = useState("");
  const [status, setStatus] = useState("Pick a cortex type, then choose the folder where it lives.");
  const [pending, setPending] = useState(false);

  useEffect(() => {
    setNetworks(readNetworks());
  }, []);

  const canCreate = useMemo(() => {
    if (pending) return false;
    return Boolean(folderPath.trim());
  }, [folderPath, pending]);

  async function persistAndOpen(network: NetworkRecord) {
    // Non-demo networks split into two regimes today:
    // - knowledge-graph: still DB-backed; configure + ingest path.
    // - lif/hh: folder-backed; open the specific `.cortex/` root.
    // We keep persisting locally even if core is offline so the shell
    // doesn't lose the user's network list.
    if (network.origin !== "demo") {
      if (network.cortexType === "knowledge-graph") {
        await pushCortexTypeToCore(network.cortexType, network.hhConfig ?? null);
      } else {
        try {
          await openCortexFolder(network.folderPath);
        } catch (err) {
          console.warn("openCortexFolder failed; falling back to configure", err);
          await pushCortexTypeToCore(network.cortexType, network.hhConfig ?? null);
        }
      }
    }
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
        if (info.metadata?.cortex_type) {
          existing.cortexType = info.metadata.cortex_type as CortexTypeSlug;
          existing.origin = existing.cortexType;
        }
      }
      await persistAndOpen(existing);
      return;
    }

    const info = await inspectCortexFolder(selected);
    if (info?.has_cortex && info.metadata) {
      const detected = (info.metadata.cortex_type as CortexTypeSlug) ?? "lif";
      await persistAndOpen({
        id: info.metadata.id ?? `existing-${Date.now()}`,
        name: info.metadata.name ?? `${nameFromPath(selected, "Existing")} Cortex`,
        origin: detected,
        cortexType: detected,
        hhConfig: info.metadata.hh_config ?? null,
        folderPath: selected,
        hasMetadata: true,
        createdAt: info.metadata.created_at ?? new Date().toISOString(),
        lastOpenedAt: new Date().toISOString(),
      });
      return;
    }

    setFolderPath(selected);
    if (info) {
      setStatus("No .cortex/ metadata found. Pick a cortex type and initialize.");
    } else if (!isTauriRuntime()) {
      setStatus("Folder selected. Pick a cortex type and initialize.");
    } else {
      setStatus("Folder selected. Couldn't inspect — pick a type and initialize.");
    }
  }

  async function createNetwork() {
    if (!canCreate) return;

    setPending(true);
    const now = new Date().toISOString();
    const hhConfig: HhConfigOverrides | null =
      cortexType === "hh" ? { integrator: hhIntegrator } : null;
    const network: NetworkRecord = {
      id: `${cortexType}-${Date.now()}`,
      name:
        cortexType === "knowledge-graph"
          ? `${nameFromPath(folderPath, "Knowledge Graph")} Cortex`
          : `${nameFromPath(folderPath, "Untitled")} Cortex`,
      origin: cortexType,
      cortexType,
      hhConfig,
      folderPath: folderPath.trim(),
      hasMetadata: false,
      createdAt: now,
      lastOpenedAt: now,
    };

    try {
      // 1. Reconfigure the engine first. If this fails the network
      //    still persists locally (core may be starting), but we surface
      //    the state to the user so they know the visualizer might run
      //    on the previous network's neuron kind until core is reached.
      const engineReady = await pushCortexTypeToCore(cortexType, hhConfig);
      if (!engineReady) {
        setStatus("Engine reconfigure failed — will retry on open.");
      }

      // 2. KG networks ingest the vault into the engine. LIF/HH stay
      //    empty so the user can stimulate manually or build topology.
      if (cortexType === "knowledge-graph") {
        setStatus("Ingesting folder contents into the neural graph...");
        const summary = await ingestVault(network.folderPath);
        network.nodeEstimate = summary.total_nodes;
        network.edgeEstimate = summary.total_edges;
        setStatus(`Created ${summary.total_nodes} nodes and ${summary.total_edges} edges.`);
      } else if (cortexType === "hh") {
        setStatus(`Initializing fresh Hodgkin-Huxley cortex (${hhIntegrator.toUpperCase()})...`);
      } else {
        setStatus("Initializing fresh LIF cortex...");
      }

      // 3. Write .cortex/ — durable provenance for future opens.
      const info = await initCortexFolder(
        network.folderPath,
        network.name,
        cortexType,
        hhConfig,
      );
      if (info?.has_cortex) {
        network.hasMetadata = true;
        network.id = info.metadata?.id ?? network.id;
        const where = info.metadata?.cortex_type ?? cortexType;
        setStatus(
          cortexType === "knowledge-graph"
            ? `Neural network initialized · ${network.nodeEstimate ?? 0} nodes · .cortex/ written (${where})`
            : `Fresh ${where.toUpperCase()} cortex initialized · .cortex/ written`,
        );
      }

      await persistAndOpen(network);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`Network shell created; core call failed: ${message}`);
      await persistAndOpen(network);
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
                  onClick={() => { void persistAndOpen(network); }}
                >
                  <div className="network-card-top mono">
                    <span>{network.cortexType.replace("-", " ")}</span>
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
            <div className="start-section-hd mono">new network — pick a cortex type</div>

            <div className="cortex-type-grid" role="radiogroup" aria-label="Cortex type">
              {CORTEX_TYPE_CARDS.map((card) => (
                <button
                  key={card.slug}
                  type="button"
                  role="radio"
                  aria-checked={cortexType === card.slug}
                  className={`cortex-type-card ${cortexType === card.slug ? "active" : ""}`}
                  onClick={() => setCortexType(card.slug)}
                >
                  <div className="cortex-type-title">{card.title}</div>
                  <div className="cortex-type-blurb">{card.blurb}</div>
                  <div className="cortex-type-slug mono">{card.slug}</div>
                </button>
              ))}
            </div>

            {cortexType === "hh" && (
              <div className="hh-config" role="radiogroup" aria-label="HH integrator">
                <span className="mono">integrator</span>
                <label>
                  <input
                    type="radio"
                    name="hh-integrator"
                    checked={hhIntegrator === "euler"}
                    onChange={() => setHhIntegrator("euler")}
                  />
                  Euler (default; needs dt ≤ 0.01 ms)
                </label>
                <label>
                  <input
                    type="radio"
                    name="hh-integrator"
                    checked={hhIntegrator === "rk4"}
                    onChange={() => setHhIntegrator("rk4")}
                  />
                  RK4 (stable to ~0.05 ms; ~4× cost)
                </label>
              </div>
            )}

            <div className="start-form">
              <label className="path-picker">
                <span className="mono">cortex folder</span>
                <div>
                  <input value={folderPath} onChange={(e) => setFolderPath(e.target.value)} />
                  <button type="button" onClick={chooseFolder}>Choose</button>
                </div>
              </label>

              <button
                className="start-primary"
                type="button"
                disabled={!canCreate}
                onClick={() => { void createNetwork(); }}
              >
                {pending ? "Creating..." : "Create and Open"}
              </button>
            </div>
          </section>
        </div>
      </section>
    </main>
  );
}
