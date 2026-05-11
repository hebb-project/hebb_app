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
