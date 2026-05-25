"use client";

import { useEffect, useMemo, useState } from "react";
import {
  configureCortex,
  CortexTypeBody,
  ingestVault,
  OpenCortexResponse,
  openCortexFolder,
} from "@/lib/cortex-api";
import {
  CortexTypeSlug,
  HhConfigOverrides,
  initCortexFolder,
  inspectCortexFolder,
  isTauriRuntime,
  pickDirectory,
  SeedKind,
  SeedSpec,
  seedCortexFolder,
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
  folderStatus?: "unknown" | "available" | "missing";
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
  folderStatus: "available",
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
          folderStatus: item.folderStatus ?? "unknown",
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

function formatOpenError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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

function sortByRecent(a: NetworkRecord, b: NetworkRecord): number {
  return Date.parse(b.lastOpenedAt) - Date.parse(a.lastOpenedAt);
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

/**
 * Seed-network UI state. Only used when `cortexType ∈ {lif, hh}`; KG
 * networks always derive topology from the source folder.
 */
type SeedUiState = {
  enabled: boolean;
  kind: SeedKind;
  // Per-kind params. Kept as a single bag so switching kinds keeps the
  // user's previous picks visible — UX nicety, no functional impact.
  n: number;
  k: number;
  p: number;
  pRewire: number;
  layers: string; // comma-separated, parsed at submit time
  seedValue: number;
};

/**
 * Fresh LIF/HH networks default to a seeded random topology (32 neurons,
 * p=0.1) so the user opens into something visible rather than an empty
 * canvas. The `enabled` flag can be toggled off explicitly to create a
 * genuinely empty network for advanced use (build-mode or agent-driven
 * topology construction).
 */
const DEFAULT_SEED_UI: SeedUiState = {
  enabled: true,
  kind: "random",
  n: 32,
  k: 3,
  p: 0.1,
  pRewire: 0.2,
  layers: "8, 16, 8",
  seedValue: 0,
};

/**
 * Translate the UI bag into the Tauri-facing `SeedSpec`. Returns
 * either the spec or a human-readable error suitable for the status
 * line. Validation is intentionally minimal — the Rust side is the
 * real gate and returns substrate-quality error text.
 */
function buildSeedSpec(ui: SeedUiState): SeedSpec | { error: string } {
  const config = { seed: ui.seedValue };
  switch (ui.kind) {
    case "random":
      return { kind: "random", n: ui.n, p: ui.p, config };
    case "ring":
      return { kind: "ring", n: ui.n, k: ui.k, config };
    case "small_world":
      return {
        kind: "small_world",
        n: ui.n,
        k: ui.k,
        p_rewire: ui.pRewire,
        config,
      };
    case "layered": {
      const layers = ui.layers
        .split(",")
        .map((s) => Number.parseInt(s.trim(), 10))
        .filter((n) => Number.isFinite(n));
      if (layers.length < 2) {
        return { error: "layered needs at least 2 layer sizes (e.g. \"8, 16, 8\")" };
      }
      if (layers.some((n) => n <= 0)) {
        return { error: "every layer size must be a positive integer" };
      }
      return { kind: "layered", layers, config };
    }
  }
}

export function StartScreen({ onOpen }: Props) {
  const [networks, setNetworks] = useState<NetworkRecord[]>([DEMO_NETWORK]);
  const [cortexType, setCortexType] = useState<CortexTypeSlug>("knowledge-graph");
  const [hhIntegrator, setHhIntegrator] = useState<"euler" | "rk4">("euler");
  const [folderPath, setFolderPath] = useState("");
  const [seedUi, setSeedUi] = useState<SeedUiState>(DEFAULT_SEED_UI);
  const [status, setStatus] = useState("Pick a cortex type, then choose the folder where it lives.");
  const [pending, setPending] = useState(false);

  useEffect(() => {
    setNetworks(readNetworks());
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const candidates = readNetworks().filter((network) => network.origin !== "demo");
    if (candidates.length === 0) return;
    let cancelled = false;
    const refreshStatuses = async () => {
      const inspected = await Promise.all(
        candidates.map(async (network) => {
          const info = await inspectCortexFolder(network.folderPath);
          const next: NetworkRecord = {
            ...network,
            folderStatus: info ? "available" : "missing",
          };
          if (info) {
            next.hasMetadata = info.has_cortex;
            if (info.metadata?.cortex_type) {
              next.cortexType = info.metadata.cortex_type as CortexTypeSlug;
              next.origin = next.cortexType;
            }
            if (info.node_count !== null) next.nodeEstimate = info.node_count;
            if (info.edge_count !== null) next.edgeEstimate = info.edge_count;
          }
          return next;
        }),
      );
      if (cancelled) return;
      const byId = new Map(inspected.map((network) => [network.id, network]));
      setNetworks((current) => {
        const next = current.map((network) => byId.get(network.id) ?? network);
        writeNetworks(next);
        return next;
      });
    };
    void refreshStatuses();
    return () => {
      cancelled = true;
    };
  }, []);

  const canCreate = useMemo(() => {
    if (pending) return false;
    return Boolean(folderPath.trim());
  }, [folderPath, pending]);

  const sortedNetworks = useMemo(
    () => [...networks].sort(sortByRecent),
    [networks],
  );
  const recentNetworks = sortedNetworks.slice(0, 2);
  const historyNetworks = sortedNetworks.slice(2);

  function saveNetworkRecord(network: NetworkRecord) {
    const updated = [
      { ...network, lastOpenedAt: new Date().toISOString(), folderStatus: "available" as const },
      ...networks.filter((item) => item.id !== network.id),
    ];
    setNetworks(updated);
    writeNetworks(updated);
    return updated[0];
  }

  async function persistAndOpen(network: NetworkRecord) {
    if (network.folderStatus === "missing") {
      setStatus(`${network.name} folder was moved or deleted.`);
      return;
    }
    // Non-demo networks split into two regimes today:
    // - knowledge-graph: still DB-backed; configure + ingest path.
    // - lif/hh: folder-backed; opening the specific `.cortex/` root is
    //   required. Falling back to bare configure would detach core from
    //   the folder and make multiple local networks appear shared.
    let opened: OpenCortexResponse | null = null;
    if (network.origin !== "demo") {
      if (network.cortexType === "knowledge-graph") {
        await pushCortexTypeToCore(network.cortexType, network.hhConfig ?? null);
      } else {
        try {
          opened = await openCortexFolder(network.folderPath);
        } catch (err) {
          const message = formatOpenError(err);
          console.warn("openCortexFolder failed", err);
          setStatus(`Could not open folder-backed ${network.cortexType} cortex: ${message}`);
          throw err;
        }
      }
    }

    const openedType = opened?.cortex_type as CortexTypeSlug | undefined;
    const hydrated: NetworkRecord = opened
      ? {
          ...network,
          name: opened.name || network.name,
          cortexType: openedType ?? network.cortexType,
          origin: openedType ?? network.origin,
          nodeEstimate: opened.n_nodes,
          edgeEstimate: opened.n_edges,
          hasMetadata: true,
        }
      : network;
    const saved = saveNetworkRecord(hydrated);
    if (opened) {
      setStatus(
        `Opened ${opened.cortex_type.toUpperCase()} cortex · ${opened.n_nodes} nodes · ${opened.n_edges} edges`,
      );
    }
    onOpen(saved);
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
      // Re-inspect so the metadata flag and topology counts reflect the
      // on-disk truth rather than cached values from when the folder was
      // first added. This is what persists nodeEstimate/edgeEstimate from
      // the actual topology.json.
      const info = await inspectCortexFolder(selected);
      const refreshed: NetworkRecord = {
        ...existing,
        folderStatus: info || !isTauriRuntime() ? "available" : "missing",
      };
      if (info) {
        refreshed.hasMetadata = info.has_cortex;
        if (info.metadata?.cortex_type) {
          refreshed.cortexType = info.metadata.cortex_type as CortexTypeSlug;
          refreshed.origin = refreshed.cortexType;
        }
        if (info.node_count !== null) refreshed.nodeEstimate = info.node_count;
        if (info.edge_count !== null) refreshed.edgeEstimate = info.edge_count;
      }
      try {
        await persistAndOpen(refreshed);
      } catch {
        // Status is set by persistAndOpen; keep the start screen active.
      }
      return;
    }

    const info = await inspectCortexFolder(selected);
    if (info?.has_cortex && info.metadata) {
      const detected = (info.metadata.cortex_type as CortexTypeSlug) ?? "lif";
      try {
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
          // Persist topology counts so the network list reflects what's
          // actually in the folder rather than "0 nodes" defaults.
          nodeEstimate: info.node_count ?? undefined,
          edgeEstimate: info.edge_count ?? undefined,
          folderStatus: "available",
        });
      } catch {
        // Status is set by persistAndOpen; keep the start screen active.
      }
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
      folderStatus: "available",
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

      // 4. Optional seed pass (LIF/HH only; KG ingested its topology
      //    in step 2). The Rust side is the real validator — any
      //    failure here is reported in the status line but doesn't
      //    block opening the (empty) network.
      if (cortexType !== "knowledge-graph" && seedUi.enabled && info?.has_cortex) {
        const built = buildSeedSpec(seedUi);
        if ("error" in built) {
          setStatus(`Seed skipped: ${built.error}`);
        } else {
          try {
            const summary = await seedCortexFolder(network.folderPath, built);
            if (summary) {
              network.nodeEstimate = summary.added_nodes;
              network.edgeEstimate = summary.added_edges;
              setStatus(
                `Seeded ${seedUi.kind.replace("_", "-")} network · ${summary.added_nodes} nodes · ${summary.added_edges} edges`,
              );
            }
          } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            setStatus(`Seed failed: ${message}`);
          }
        }
      }

      await persistAndOpen(network);
    } catch (error) {
      const message = formatOpenError(error);
      if (network.hasMetadata) {
        saveNetworkRecord(network);
        setStatus(`Network folder was saved, but core could not open it: ${message}`);
      } else {
        setStatus(`Network creation failed: ${message}`);
      }
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
            <div className="start-section-subhd mono">recent</div>
            <div className="network-recent-list">
              {recentNetworks.map((network) => (
                <button
                  key={network.id}
                  className={`network-card ${network.folderStatus === "missing" ? "missing" : ""}`}
                  onClick={() => {
                    void persistAndOpen(network).catch(() => {
                      // `persistAndOpen` already surfaces the actionable
                      // status. Stay on the start screen instead of
                      // showing stale graph state from the previous folder.
                    });
                  }}
                >
                  <div className="network-card-top mono">
                    <span>{network.cortexType.replace("-", " ")}</span>
                    <span>{network.nodeEstimate ?? 0} nodes</span>
                  </div>
                  <div className="network-card-name">{network.name}</div>
                  <div className="network-card-path mono">{shortPath(network.folderPath)}</div>
                  <div className="network-card-path mono network-card-status">
                    {network.folderStatus === "missing"
                      ? "⚠ folder moved/deleted"
                      : network.hasMetadata ? "metadata detected" : "metadata pending"}
                  </div>
                </button>
              ))}
            </div>

            {historyNetworks.length > 0 && (
              <>
                <div className="start-section-subhd history mono">all previous</div>
                <div className="network-history-list">
                  {historyNetworks.map((network) => (
                    <button
                      key={network.id}
                      className={`network-row ${network.folderStatus === "missing" ? "missing" : ""}`}
                      onClick={() => {
                        void persistAndOpen(network).catch(() => {
                          // Status is already set by persistAndOpen.
                        });
                      }}
                    >
                      <span className="network-row-kind mono">{network.cortexType.replace("-", " ")}</span>
                      <span className="network-row-main">
                        <span className="network-row-name">{network.name}</span>
                        <span className="network-row-path mono">{shortPath(network.folderPath)}</span>
                      </span>
                      <span className="network-row-meta mono">
                        {network.folderStatus === "missing"
                          ? "⚠ moved/deleted"
                          : `${network.nodeEstimate ?? 0} nodes`}
                      </span>
                    </button>
                  ))}
                </div>
              </>
            )}
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

            {cortexType !== "knowledge-graph" && (
              <div className="seed-config">
                {/* Primary toggle: seed on create (default on). Flipping it
                    off lets advanced users start with an empty topology for
                    build-mode or agent-driven construction. */}
                <label className="seed-toggle-row">
                  <input
                    type="checkbox"
                    checked={seedUi.enabled}
                    onChange={(e) =>
                      setSeedUi((s) => ({ ...s, enabled: e.target.checked }))
                    }
                  />
                  <span>seed starter topology on create</span>
                  {!seedUi.enabled && (
                    <span className="seed-empty-hint mono">
                      — starts empty (0 nodes)
                    </span>
                  )}
                </label>

                {seedUi.enabled && (
                  <details className="seed-advanced">
                    <summary className="mono">topology options</summary>
                    <div className="seed-config-body">
                      <label className="seed-row">
                        <span className="mono">generator</span>
                        <select
                          value={seedUi.kind}
                          onChange={(e) =>
                            setSeedUi((s) => ({ ...s, kind: e.target.value as SeedKind }))
                          }
                        >
                          <option value="random">random (Erdős-Rényi)</option>
                          <option value="ring">ring lattice</option>
                          <option value="small_world">small-world (Watts-Strogatz)</option>
                          <option value="layered">layered feed-forward</option>
                        </select>
                      </label>

                      {(seedUi.kind === "random" ||
                        seedUi.kind === "ring" ||
                        seedUi.kind === "small_world") && (
                        <label className="seed-row">
                          <span className="mono">n (neurons)</span>
                          <input
                            type="number"
                            min={2}
                            value={seedUi.n}
                            onChange={(e) =>
                              setSeedUi((s) => ({ ...s, n: Number(e.target.value) }))
                            }
                          />
                        </label>
                      )}

                      {seedUi.kind === "random" && (
                        <label className="seed-row">
                          <span className="mono">p (edge prob)</span>
                          <input
                            type="number"
                            min={0}
                            max={1}
                            step={0.01}
                            value={seedUi.p}
                            onChange={(e) =>
                              setSeedUi((s) => ({ ...s, p: Number(e.target.value) }))
                            }
                          />
                        </label>
                      )}

                      {(seedUi.kind === "ring" || seedUi.kind === "small_world") && (
                        <label className="seed-row">
                          <span className="mono">k (neighbors each side)</span>
                          <input
                            type="number"
                            min={1}
                            value={seedUi.k}
                            onChange={(e) =>
                              setSeedUi((s) => ({ ...s, k: Number(e.target.value) }))
                            }
                          />
                        </label>
                      )}

                      {seedUi.kind === "small_world" && (
                        <label className="seed-row">
                          <span className="mono">p_rewire</span>
                          <input
                            type="number"
                            min={0}
                            max={1}
                            step={0.01}
                            value={seedUi.pRewire}
                            onChange={(e) =>
                              setSeedUi((s) => ({ ...s, pRewire: Number(e.target.value) }))
                            }
                          />
                        </label>
                      )}

                      {seedUi.kind === "layered" && (
                        <label className="seed-row">
                          <span className="mono">layers (csv)</span>
                          <input
                            type="text"
                            value={seedUi.layers}
                            onChange={(e) =>
                              setSeedUi((s) => ({ ...s, layers: e.target.value }))
                            }
                          />
                        </label>
                      )}

                      <label className="seed-row">
                        <span className="mono">prng seed</span>
                        <input
                          type="number"
                          value={seedUi.seedValue}
                          onChange={(e) =>
                            setSeedUi((s) => ({ ...s, seedValue: Number(e.target.value) }))
                          }
                        />
                      </label>
                    </div>
                  </details>
                )}
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
