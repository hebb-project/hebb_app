// Browser-side client for the Rust core. Keeps URL handling and
// envelope-unwrapping in one place.

export type CortexNode = {
  id: string;
  label: string;
  node_type: string;
  source_file: string | null;
  metadata: Record<string, unknown>;
};

export type CortexEdge = {
  id: string;
  pre_id: string;
  post_id: string;
  weight: number;
  edge_type: string;
};

export type NewCortexNode = {
  label: string;
  node_type?: string;
  source_file?: string | null;
  model_blob_path?: string | null;
  metadata?: Record<string, unknown>;
};

export type NewCortexEdge = {
  pre_id: string;
  post_id: string;
  weight?: number;
  edge_type?: string;
  metadata?: Record<string, unknown>;
};

export type CortexSpikeEvent = { node_id: string; t_ms: number };
export type CortexSpikeFrame = { v: number; t_ms: number; events: CortexSpikeEvent[] };

export type CortexWeightDelta = { edge_id: string; w: number };
export type CortexWeightFrame = {
  v: number;
  t_ms: number;
  full: boolean;
  deltas: CortexWeightDelta[];
};

export type CortexVoltageSample = { node_id: string; v_mV: number };
export type CortexVoltageFrame = {
  v: number;
  t_ms: number;
  samples: CortexVoltageSample[];
};

export type CortexParams = Record<string, unknown>;

export type CortexSearchResult = {
  node_id: string;
  label: string;
  node_type: string;
  source_file: string | null;
  score: number;
  snippet: string | null;
  highlights: string[];
};

export type CortexSearchResponse = {
  query: string;
  results: CortexSearchResult[];
};

export type VaultIngestSummary = {
  files_seen: number;
  nodes_created: number;
  nodes_existing: number;
  edges_created: number;
  edges_existing: number;
  total_nodes: number;
  total_edges: number;
};

// Must match DEFAULT_BIND in desktop/src-tauri/src/supervisor/core.rs and default_ws_port in core/src/config.rs.
const DEFAULT_HTTP =
  process.env.NEXT_PUBLIC_CORTEX_HTTP ?? "http://127.0.0.1:7654";

export function cortexHttpBase(override?: string): string {
  return override?.replace(/\/$/, "") ?? DEFAULT_HTTP;
}

export function cortexWsUrl(override?: string): string {
  if (override) return override;
  const http = cortexHttpBase();
  return http.replace(/^http/, "ws") + "/ws/spikes";
}

export function cortexWeightsWsUrl(override?: string): string {
  if (override) return override;
  const http = cortexHttpBase();
  return http.replace(/^http/, "ws") + "/ws/weights";
}

/// Voltage stream URL. `nodes` filters to specific neuron ids; omit to
/// sample all (server caps the unfiltered set).
export function cortexVoltageWsUrl(nodes?: string[], override?: string): string {
  const base = override ?? cortexHttpBase().replace(/^http/, "ws") + "/ws/voltage";
  if (nodes && nodes.length > 0) {
    const q = encodeURIComponent(nodes.join(","));
    return `${base}?nodes=${q}`;
  }
  return base;
}

type CortexErrorBody = {
  data: unknown;
  error: null | { code?: string; message?: string } | unknown;
};

async function unwrap<T>(r: Response): Promise<T> {
  let body: CortexErrorBody | null = null;
  try {
    body = (await r.json()) as CortexErrorBody;
  } catch {
    // Keep the status-only fallback for non-JSON failures.
  }

  if (!r.ok) {
    const error = body?.error;
    const message =
      error && typeof error === "object" && "message" in error
        ? String((error as { message?: unknown }).message)
        : `cortex http ${r.status}`;
    throw new Error(message);
  }
  if (body?.error) {
    const error = body.error;
    const message =
      error && typeof error === "object" && "message" in error
        ? String((error as { message?: unknown }).message)
        : JSON.stringify(error);
    throw new Error(message);
  }
  if (!body) throw new Error("cortex response was not JSON");
  return body.data as T;
}

export async function fetchGraph(base?: string): Promise<{ nodes: CortexNode[]; edges: CortexEdge[] }> {
  const r = await fetch(`${cortexHttpBase(base)}/api/graph`, { cache: "no-store" });
  return unwrap(r);
}

export async function createGraphNode(
  node: NewCortexNode,
  base?: string,
): Promise<CortexNode> {
  const r = await fetch(`${cortexHttpBase(base)}/api/graph/nodes`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(node),
  });
  return unwrap(r);
}

export async function createGraphEdge(
  edge: NewCortexEdge,
  base?: string,
): Promise<CortexEdge> {
  const r = await fetch(`${cortexHttpBase(base)}/api/graph/edges`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(edge),
  });
  return unwrap(r);
}

export async function searchVault(
  query: string,
  limit = 8,
  base?: string,
): Promise<CortexSearchResponse> {
  const params = new URLSearchParams({ q: query, limit: String(limit) });
  const r = await fetch(`${cortexHttpBase(base)}/api/search?${params}`, { cache: "no-store" });
  return unwrap(r);
}

export async function ingestVault(path?: string, base?: string): Promise<VaultIngestSummary> {
  const r = await fetch(`${cortexHttpBase(base)}/api/vault/ingest`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(path?.trim() ? { path: path.trim() } : {}),
  });
  return unwrap(r);
}

export type CortexChatStimulus = {
  node_id: string;
  label: string;
  current: number;
  duration_ms: number;
  score: number;
};

export type CortexChatActivation = {
  node_id: string;
  label: string;
  spike_count: number;
};

export type CortexChatResponse = {
  reply: string;
  encoder: string;
  stimulated: CortexChatStimulus[];
  activated: CortexChatActivation[];
};

/**
 * Hit `/api/chat` on the Rust core. The desktop ships this in-process
 * — no Python bridge required. Mirrors the bridge's response shape so
 * the UI is decoupled from which transport answered.
 */
export async function postChat(
  message: string,
  base?: string,
): Promise<CortexChatResponse> {
  const r = await fetch(`${cortexHttpBase(base)}/api/chat`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ message }),
  });
  return unwrap(r);
}

/**
 * Body shape for `POST /api/cortex` — mirrors the kebab-tagged
 * `core::CortexType` enum. The Rust side wipes engine state on receipt
 * and re-seeds with the requested neuron kind.
 */
export type CortexTypeBody =
  | { kind: "knowledge-graph" }
  | { kind: "lif" }
  | { kind: "hh"; config?: { integrator?: "euler" | "rk4" } };

export type CortexTypeStatus = CortexTypeBody;

/**
 * Switch the engine to a new cortex type. Destructive — the actor
 * drops all in-memory neurons/synapses before re-seeding. Caller is
 * expected to re-ingest topology afterward (or skip it for `hh`/`lif`
 * fresh networks).
 */
export async function configureCortex(
  body: CortexTypeBody,
  base?: string,
): Promise<{ cortex_type: string }> {
  const r = await fetch(`${cortexHttpBase(base)}/api/cortex`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return unwrap(r);
}

export async function getCortexType(base?: string): Promise<CortexTypeStatus> {
  const r = await fetch(`${cortexHttpBase(base)}/api/cortex`, { cache: "no-store" });
  return unwrap(r);
}

export type OpenCortexResponse = {
  folder: string;
  cortex_type: string;
  name: string;
  n_nodes: number;
  n_edges: number;
  weights_loaded: number;
};

export type CortexFolderStatus = {
  folder: string | null;
};

/**
 * Hydrate core from a folder-backed `.cortex` network. Used for
 * non-KG networks where the folder, not Postgres, is the structural
 * source of truth.
 */
function cortexDataFolder(root: string): string {
  const trimmed = root.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/);
  if (parts.at(-1) === ".cortex") return trimmed;
  return `${trimmed}/.cortex`;
}

export async function openCortexFolder(
  rootFolder: string,
  base?: string,
): Promise<OpenCortexResponse> {
  const r = await fetch(`${cortexHttpBase(base)}/api/cortex/open`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ folder: cortexDataFolder(rootFolder) }),
  });
  return unwrap(r);
}

export async function getCortexFolder(base?: string): Promise<CortexFolderStatus> {
  const r = await fetch(`${cortexHttpBase(base)}/api/cortex/folder`, { cache: "no-store" });
  return unwrap(r);
}

export async function postStimulate(
  nodeId: string,
  current: number,
  durationMs: number,
  base?: string,
): Promise<void> {
  const r = await fetch(`${cortexHttpBase(base)}/api/stimulate`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ node_id: nodeId, current, duration_ms: durationMs }),
  });
  await unwrap(r);
}

export async function fetchNodeParams(nodeId: string, base?: string): Promise<CortexParams> {
  const r = await fetch(`${cortexHttpBase(base)}/api/nodes/${nodeId}/params`, {
    cache: "no-store",
  });
  return unwrap(r);
}

export async function fetchSynapseParams(edgeId: string, base?: string): Promise<CortexParams> {
  const r = await fetch(`${cortexHttpBase(base)}/api/synapses/${edgeId}/params`, {
    cache: "no-store",
  });
  return unwrap(r);
}

export async function patchNodeParam(
  nodeId: string,
  key: string,
  value: unknown,
  base?: string,
): Promise<CortexParams> {
  const r = await fetch(`${cortexHttpBase(base)}/api/nodes/${nodeId}/params`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ key, value }),
  });
  return unwrap(r);
}

export async function patchSynapseParam(
  edgeId: string,
  key: string,
  value: unknown,
  base?: string,
): Promise<CortexParams> {
  const r = await fetch(`${cortexHttpBase(base)}/api/synapses/${edgeId}/params`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ key, value }),
  });
  return unwrap(r);
}
