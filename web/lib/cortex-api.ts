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

export type CortexSpikeEvent = { node_id: string; t_ms: number };
export type CortexSpikeFrame = { v: number; t_ms: number; events: CortexSpikeEvent[] };

export type CortexWeightDelta = { edge_id: string; w: number };
export type CortexWeightFrame = {
  v: number;
  t_ms: number;
  full: boolean;
  deltas: CortexWeightDelta[];
};

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

const DEFAULT_HTTP =
  process.env.NEXT_PUBLIC_CORTEX_HTTP ?? "http://127.0.0.1:8080";

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

async function unwrap<T>(r: Response): Promise<T> {
  if (!r.ok) throw new Error(`cortex http ${r.status}`);
  const body = (await r.json()) as { data: T; error: unknown };
  if (body.error) throw new Error(`cortex error: ${JSON.stringify(body.error)}`);
  return body.data;
}

export async function fetchGraph(base?: string): Promise<{ nodes: CortexNode[]; edges: CortexEdge[] }> {
  const r = await fetch(`${cortexHttpBase(base)}/api/graph`, { cache: "no-store" });
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
