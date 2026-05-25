"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { STATE_PRESETS, type Palette, type StateKey } from "@/lib/state";
import {
  cortexHttpBase,
  cortexVoltageWsUrl,
  cortexWeightsWsUrl,
  cortexWsUrl,
  createGraphEdge,
  createGraphNode,
  fetchNodeParams,
  fetchGraph,
  fetchSynapseParams,
  patchNodeParam,
  patchSynapseParam,
  postStimulate,
  type CortexEdge,
  type CortexNode,
  type CortexParams,
  type CortexSpikeFrame,
  type CortexVoltageFrame,
  type CortexWeightFrame,
} from "@/lib/cortex-api";

type GraphNode = {
  i: number;
  id?: string;          // uuid in live mode
  label?: string;       // human label in live mode
  nodeType?: string;
  x: number;
  y: number;
  r: number;
  hot: boolean;
  fire: number;
  hotFire: number;
  neighbors: number[];
};

type GraphEdge = {
  a: number;
  b: number;
  len: number;
  longRange?: boolean;
  id?: string;
  weight: number;
  // Transient STDP-update pulse. `pulse` decays toward 0 each frame;
  // `pulseDir` is +1 for potentiation (Δw > 0) and -1 for depression so
  // the renderer can tint the moment of plasticity differently from the
  // settled steady-state weight.
  pulse: number;
  pulseDir: number;
};

type Graph = {
  nodes: GraphNode[];
  edges: GraphEdge[];
  idToIndex: Map<string, number>;
  edgeIdToIndex: Map<string, number>;
};

type Spike = { from: number; to: number; t0: number; dur: number };

type BuildDraft = {
  fromId: string;
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
} | null;

type ParamTarget =
  | { kind: "node"; id: string; label?: string }
  | { kind: "synapse"; id: string; label?: string };

type HitTarget =
  | { kind: "node"; id: string; label?: string }
  | { kind: "synapse"; id: string; label?: string };

type Colors = {
  bg: string;
  gridLine: string;
  edge: string;
  edgeStrong: string;
  cyan: string;
  pink: string;
  nodeIdle: string;
};

type View = { scale: number; tx: number; ty: number };

const MIN_SCALE = 0.15;
const MAX_SCALE = 12;

// ── Graph builders ────────────────────────────────────────────────────────

function buildConnectome(nodeCount: number, w: number, h: number, seed = 7): Graph {
  let s = seed >>> 0;
  const rand = () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0xffffffff;
  };

  const clusters = [
    { cx: 0.32, cy: 0.42, r: 0.22, w: 0.32 },
    { cx: 0.62, cy: 0.36, r: 0.2, w: 0.26 },
    { cx: 0.48, cy: 0.68, r: 0.24, w: 0.28 },
    { cx: 0.78, cy: 0.62, r: 0.16, w: 0.14 },
  ];
  const pickCluster = () => {
    const r = rand();
    let acc = 0;
    for (const c of clusters) {
      acc += c.w;
      if (r <= acc) return c;
    }
    return clusters[clusters.length - 1];
  };

  const nodes: GraphNode[] = [];
  for (let i = 0; i < nodeCount; i++) {
    const c = pickCluster();
    const u1 = (rand() + rand()) * 0.5 - 0.5;
    const u2 = (rand() + rand()) * 0.5 - 0.5;
    const x = (c.cx + u1 * c.r * 1.6) * w;
    const y = (c.cy + u2 * c.r * 1.6) * h;
    const hot = rand() < 0.08;
    nodes.push({
      i,
      x: Math.max(20, Math.min(w - 20, x)),
      y: Math.max(20, Math.min(h - 20, y)),
      r: 1.4 + rand() * 1.6,
      hot,
      fire: 0,
      hotFire: 0,
      neighbors: [],
    });
  }

  const edges: GraphEdge[] = [];
  const seen = new Set<string>();
  const k = 3;
  for (let i = 0; i < nodes.length; i++) {
    const a = nodes[i];
    const dists: { j: number; d: number }[] = [];
    for (let j = 0; j < nodes.length; j++) {
      if (j === i) continue;
      const b = nodes[j];
      const dx = a.x - b.x;
      const dy = a.y - b.y;
      dists.push({ j, d: dx * dx + dy * dy });
    }
    dists.sort((p, q) => p.d - q.d);
    for (let t = 0; t < k; t++) {
      const j = dists[t].j;
      const key = i < j ? `${i}-${j}` : `${j}-${i}`;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ a: i, b: j, len: Math.sqrt(dists[t].d), weight: 0.5, pulse: 0, pulseDir: 0 });
      nodes[i].neighbors.push(j);
      nodes[j].neighbors.push(i);
    }
  }

  const longRange = Math.max(4, Math.floor(nodeCount * 0.04));
  for (let t = 0; t < longRange; t++) {
    const i = Math.floor(rand() * nodes.length);
    const j = Math.floor(rand() * nodes.length);
    if (i === j) continue;
    const key = i < j ? `${i}-${j}` : `${j}-${i}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const dx = nodes[i].x - nodes[j].x;
    const dy = nodes[i].y - nodes[j].y;
    edges.push({ a: i, b: j, len: Math.sqrt(dx * dx + dy * dy), longRange: true, weight: 0.5, pulse: 0, pulseDir: 0 });
    nodes[i].neighbors.push(j);
    nodes[j].neighbors.push(i);
  }

  return { nodes, edges, idToIndex: new Map(), edgeIdToIndex: new Map() };
}

/**
 * Lay out a real graph from the API. Nodes are grouped into spatial
 * clusters by `node_type` (concept / entity / log / stub / ...) so the
 * visualization stays readable as the vault grows.
 */
function buildFromApi(
  apiNodes: CortexNode[],
  apiEdges: CortexEdge[],
  w: number,
  h: number,
  seed = 11,
): Graph {
  let s = seed >>> 0;
  const rand = () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0xffffffff;
  };

  // Bucket by node_type, then assign a cluster center per bucket.
  const buckets = new Map<string, CortexNode[]>();
  for (const n of apiNodes) {
    const k = n.node_type || "concept";
    if (!buckets.has(k)) buckets.set(k, []);
    buckets.get(k)!.push(n);
  }
  const types = Array.from(buckets.keys());
  const centers = new Map<string, { cx: number; cy: number; r: number }>();
  types.forEach((t, idx) => {
    // Lay clusters around the unit circle.
    const theta = (idx / Math.max(1, types.length)) * Math.PI * 2;
    const cx = 0.5 + Math.cos(theta) * 0.28;
    const cy = 0.5 + Math.sin(theta) * 0.28;
    centers.set(t, { cx, cy, r: 0.16 });
  });

  // Pre-assign indices in deterministic order so the WS spike events
  // can find their target reliably across renders.
  const nodes: GraphNode[] = [];
  const idToIndex = new Map<string, number>();
  const ordered = [...apiNodes].sort((a, b) => a.id.localeCompare(b.id));
  for (const n of ordered) {
    const c = centers.get(n.node_type) ?? { cx: 0.5, cy: 0.5, r: 0.2 };
    const u1 = (rand() + rand()) * 0.5 - 0.5;
    const u2 = (rand() + rand()) * 0.5 - 0.5;
    const metadataX = typeof n.metadata?.x === "number" ? n.metadata.x : null;
    const metadataY = typeof n.metadata?.y === "number" ? n.metadata.y : null;
    const x = metadataX ?? (c.cx + u1 * c.r * 2) * w;
    const y = metadataY ?? (c.cy + u2 * c.r * 2) * h;
    const i = nodes.length;
    idToIndex.set(n.id, i);
    nodes.push({
      i,
      id: n.id,
      label: n.label,
      nodeType: n.node_type,
      x: Math.max(24, Math.min(w - 24, x)),
      y: Math.max(24, Math.min(h - 24, y)),
      r: 2.4 + rand() * 1.6,
      // Mark non-stub, non-entity types as "hot" for the pink highlight.
      hot: n.node_type !== "stub" && rand() < 0.18,
      fire: 0,
      hotFire: 0,
      neighbors: [],
    });
  }

  const edges: GraphEdge[] = [];
  const edgeIdToIndex = new Map<string, number>();
  for (const e of apiEdges) {
    const a = idToIndex.get(e.pre_id);
    const b = idToIndex.get(e.post_id);
    if (a === undefined || b === undefined || a === b) continue;
    const dx = nodes[a].x - nodes[b].x;
    const dy = nodes[a].y - nodes[b].y;
    const idx = edges.length;
    edges.push({
      a, b,
      len: Math.sqrt(dx * dx + dy * dy),
      longRange: e.weight > 0.75,
      id: e.id,
      weight: e.weight,
      pulse: 0,
      pulseDir: 0,
    });
    edgeIdToIndex.set(e.id, idx);
    nodes[a].neighbors.push(b);
    nodes[b].neighbors.push(a);
  }

  return { nodes, edges, idToIndex, edgeIdToIndex };
}

function parseParamInput(raw: string, current: unknown): unknown {
  if (typeof current === "number") {
    const n = Number(raw);
    if (!Number.isFinite(n)) throw new Error("expected finite number");
    return n;
  }
  if (typeof current === "boolean") return raw === "true";
  if (current === null) return raw.trim() ? raw : null;
  return raw;
}

function isParamEditable(key: string, value: unknown): boolean {
  if (key === "id" || key === "pre_id" || key === "post_id") return false;
  return value === null || ["number", "string", "boolean"].includes(typeof value);
}

function formatParamValue(value: unknown): string {
  if (value === null) return "";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function edgeDistanceSquared(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const vx = bx - ax;
  const vy = by - ay;
  const wx = px - ax;
  const wy = py - ay;
  const c1 = vx * wx + vy * wy;
  if (c1 <= 0) return (px - ax) ** 2 + (py - ay) ** 2;
  const c2 = vx * vx + vy * vy;
  if (c2 <= c1) return (px - bx) ** 2 + (py - by) ** 2;
  const t = c1 / c2;
  const qx = ax + t * vx;
  const qy = ay + t * vy;
  return (px - qx) ** 2 + (py - qy) ** 2;
}

// ── Component ─────────────────────────────────────────────────────────────

type Props = {
  stateKey: StateKey;
  nodeCount: number;
  palette: Palette;
  wireframe?: boolean;
  onSpikeRate?: (sps: number) => void;
  /** Connect to the Rust core via /api/graph + /ws/spikes. */
  live?: boolean;
  /** Override base URL (http://host:port). */
  cortexHttp?: string;
  /** Override WS URL. */
  cortexWs?: string;
  /** Current to inject on node click (live mode). */
  clickStimulusCurrent?: number;
  clickStimulusDurationMs?: number;
  /**
   * Called when the user clicks "Re-open from disk" in the empty-network
   * banner. Only shown for folder-backed networks (live mode) with 0 nodes.
   */
  onReopen?: () => void;
};

export function ConnectomeView({
  stateKey,
  nodeCount,
  palette,
  wireframe = false,
  onSpikeRate,
  live = false,
  cortexHttp,
  cortexWs,
  clickStimulusCurrent = 40,
  clickStimulusDurationMs = 400,
  onReopen,
}: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const viewRef = useRef<View>({ scale: 1, tx: 0, ty: 0 });
  const [zoomPct, setZoomPct] = useState(100);
  const [panning, setPanning] = useState(false);
  const [wsState, setWsState] = useState<"idle" | "connecting" | "open" | "closed">(
    live ? "connecting" : "idle",
  );
  const [graphMeta, setGraphMeta] = useState<{ nodes: number; edges: number } | null>(null);
  const [hoverLabel, setHoverLabel] = useState<string | null>(null);
  const [buildMode, setBuildMode] = useState(false);
  const [buildStatus, setBuildStatus] = useState("click empty space to add · drag node to node to connect");
  const [graphRevision, setGraphRevision] = useState(0);
  const [fetchFailed, setFetchFailed] = useState(false);
  // True once the graph has been fetched at least once and returned 0 nodes.
  // Cleared as soon as nodes appear (e.g. after a seed or build-mode add).
  const [graphEmpty, setGraphEmpty] = useState(false);
  const [paramTarget, setParamTarget] = useState<ParamTarget | null>(null);
  const [paramValues, setParamValues] = useState<CortexParams | null>(null);
  const [paramDrafts, setParamDrafts] = useState<Record<string, string>>({});
  const [paramStatus, setParamStatus] = useState<string>("select a node or edge");
  // Live membrane-potential trace for the selected neuron. The samples
  // accumulate in a ref (drawn imperatively to a canvas, never via React
  // state — 30 Hz reconciliation would thrash) while a small status
  // string drives the readout text.
  const voltageCanvasRef = useRef<HTMLCanvasElement>(null);
  const voltageBufRef = useRef<{ t: number; v: number }[]>([]);
  const [voltageStatus, setVoltageStatus] = useState<string>("idle");

  const colors = useMemo<Colors>(
    () => ({
      bg: "#0a0d12",
      gridLine: "rgba(31,41,51,0.35)",
      edge: "rgba(125,249,255,0.07)",
      edgeStrong: "rgba(125,249,255,0.14)",
      cyan: palette.cyan,
      pink: palette.pink,
      nodeIdle: "rgba(125,249,255,0.30)",
    }),
    [palette],
  );

  // Holds the (mutable) live graph. Populated by the fetch effect, read
  // by the animation effect via a ref so we don't re-run animation on
  // every fetch.
  const apiGraphRef = useRef<{ nodes: CortexNode[]; edges: CortexEdge[] } | null>(null);
  // Queue of node indices that the WS told us fired since the last frame.
  const pendingSpikesRef = useRef<number[]>([]);
  // Pending edge weight updates keyed by edge_id; drained into the live graph.
  const pendingWeightsRef = useRef<Map<string, number>>(new Map());
  const buildModeRef = useRef(buildMode);
  const buildDraftRef = useRef<BuildDraft>(null);

  const propsRef = useRef({ stateKey, colors, wireframe, onSpikeRate, live });
  useEffect(() => {
    propsRef.current = { stateKey, colors, wireframe, onSpikeRate, live };
  });
  useEffect(() => {
    buildModeRef.current = buildMode;
    if (!buildMode) buildDraftRef.current = null;
  }, [buildMode]);

  useEffect(() => {
    if (!live || !paramTarget) {
      setParamValues(null);
      setParamDrafts({});
      setParamStatus(live ? "select a node or edge" : "live mode required");
      return;
    }
    let cancelled = false;
    setParamStatus("loading params...");
    const load = async () => {
      try {
        const params =
          paramTarget.kind === "node"
            ? await fetchNodeParams(paramTarget.id, cortexHttp)
            : await fetchSynapseParams(paramTarget.id, cortexHttp);
        if (cancelled) return;
        setParamValues(params);
        setParamDrafts(
          Object.fromEntries(Object.entries(params).map(([key, value]) => [key, formatParamValue(value)])),
        );
        setParamStatus("ready");
      } catch (err) {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        setParamValues(null);
        setParamDrafts({});
        setParamStatus(`load failed: ${message}`);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [live, cortexHttp, paramTarget]);

  // Live V(t) trace: subscribe to /ws/voltage filtered to the selected
  // neuron, accumulate a bounded ring buffer, and draw it imperatively.
  useEffect(() => {
    voltageBufRef.current = [];
    if (!live || !paramTarget || paramTarget.kind !== "node") {
      setVoltageStatus(live ? "select a neuron" : "live mode required");
      return;
    }
    const MAX_POINTS = 600; // ~20 s at 30 Hz
    const nodeId = paramTarget.id;
    const base = (cortexHttp ?? cortexHttpBase()).replace(/^http/, "ws") + "/ws/voltage";
    const url = cortexVoltageWsUrl([nodeId], base);
    let ws: WebSocket | null = null;
    let closed = false;

    const draw = () => {
      const canvas = voltageCanvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      const w = canvas.width;
      const h = canvas.height;
      ctx.clearRect(0, 0, w, h);
      const buf = voltageBufRef.current;
      if (buf.length < 2) return;
      let lo = Infinity;
      let hi = -Infinity;
      for (const p of buf) {
        if (p.v < lo) lo = p.v;
        if (p.v > hi) hi = p.v;
      }
      // Pad a flat trace so a resting neuron doesn't render as a line on
      // the floor; keep at least a few mV of visible range.
      if (hi - lo < 4) {
        const mid = (hi + lo) / 2;
        lo = mid - 2;
        hi = mid + 2;
      }
      const pad = 6;
      const sx = (i: number) => pad + (i / (buf.length - 1)) * (w - 2 * pad);
      const sy = (v: number) => pad + (1 - (v - lo) / (hi - lo)) * (h - 2 * pad);
      ctx.beginPath();
      ctx.moveTo(sx(0), sy(buf[0].v));
      for (let i = 1; i < buf.length; i++) ctx.lineTo(sx(i), sy(buf[i].v));
      ctx.strokeStyle = "rgba(125,249,255,0.9)";
      ctx.lineWidth = 1.4;
      ctx.stroke();
    };

    setVoltageStatus("connecting...");
    try {
      ws = new WebSocket(url);
    } catch {
      setVoltageStatus("voltage stream unavailable");
      return;
    }
    ws.onopen = () => {
      if (!closed) setVoltageStatus("streaming");
    };
    ws.onmessage = (ev) => {
      let frame: CortexVoltageFrame;
      try {
        frame = JSON.parse(ev.data as string);
      } catch {
        return;
      }
      const sample = frame.samples.find((s) => s.node_id === nodeId);
      if (!sample) return;
      const buf = voltageBufRef.current;
      buf.push({ t: frame.t_ms, v: sample.v_mV });
      if (buf.length > MAX_POINTS) buf.splice(0, buf.length - MAX_POINTS);
      draw();
    };
    ws.onerror = () => {
      if (!closed) setVoltageStatus("voltage stream error");
    };
    ws.onclose = () => {
      if (!closed) setVoltageStatus("stream closed");
    };

    return () => {
      closed = true;
      ws?.close();
    };
  }, [live, cortexHttp, paramTarget]);

  const commitParam = async (key: string) => {
    if (!paramTarget || !paramValues) return;
    const current = paramValues[key];
    try {
      setParamStatus(`saving ${key}...`);
      const value = parseParamInput(paramDrafts[key] ?? "", current);
      const after =
        paramTarget.kind === "node"
          ? await patchNodeParam(paramTarget.id, key, value, cortexHttp)
          : await patchSynapseParam(paramTarget.id, key, value, cortexHttp);
      setParamValues(after);
      setParamDrafts(
        Object.fromEntries(Object.entries(after).map(([nextKey, nextValue]) => [nextKey, formatParamValue(nextValue)])),
      );
      setParamStatus(`saved ${key}`);
      if (paramTarget.kind === "synapse" && key === "weight") {
        pendingWeightsRef.current.set(paramTarget.id, Number(after.weight ?? value));
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setParamStatus(`save failed: ${message}`);
    }
  };

  // ── Trackpad zoom / pan ───────────────────────────────────────────
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;

    const clampScale = (s: number) => Math.max(MIN_SCALE, Math.min(MAX_SCALE, s));

    const zoomAt = (mx: number, my: number, factor: number) => {
      const v = viewRef.current;
      const newScale = clampScale(v.scale * factor);
      const k = newScale / v.scale;
      v.tx = mx - (mx - v.tx) * k;
      v.ty = my - (my - v.ty) * k;
      v.scale = newScale;
      setZoomPct(Math.round(newScale * 100));
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = wrap.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      if (e.ctrlKey || e.metaKey) {
        const factor = Math.exp(-e.deltaY * 0.01);
        zoomAt(mx, my, factor);
      } else {
        const v = viewRef.current;
        v.tx -= e.deltaX;
        v.ty -= e.deltaY;
      }
    };

    let dragging = false;
    let dragMoved = false;
    let lastX = 0;
    let lastY = 0;
    let spaceDown = false;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space" && !spaceDown) {
        spaceDown = true;
        setPanning(true);
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        spaceDown = false;
        setPanning(false);
      }
    };
    const onPointerDown = (e: PointerEvent) => {
      if (e.button === 1 || (e.button === 0 && spaceDown)) {
        dragging = true;
        dragMoved = false;
        lastX = e.clientX;
        lastY = e.clientY;
        wrap.setPointerCapture(e.pointerId);
        e.preventDefault();
      }
    };
    const onPointerMove = (e: PointerEvent) => {
      if (!dragging) return;
      const v = viewRef.current;
      v.tx += e.clientX - lastX;
      v.ty += e.clientY - lastY;
      lastX = e.clientX;
      lastY = e.clientY;
      dragMoved = true;
    };
    const onPointerUp = (e: PointerEvent) => {
      if (dragging) {
        dragging = false;
        try { wrap.releasePointerCapture(e.pointerId); } catch {}
      }
      // Suppress the synthetic click that follows a real pan-drag.
      if (dragMoved) {
        e.preventDefault();
        e.stopPropagation();
      }
    };

    wrap.addEventListener("wheel", onWheel, { passive: false });
    wrap.addEventListener("pointerdown", onPointerDown);
    wrap.addEventListener("pointermove", onPointerMove);
    wrap.addEventListener("pointerup", onPointerUp);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);

    return () => {
      wrap.removeEventListener("wheel", onWheel);
      wrap.removeEventListener("pointerdown", onPointerDown);
      wrap.removeEventListener("pointermove", onPointerMove);
      wrap.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, []);

  const zoomCenter = (factor: number) => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const r = wrap.getBoundingClientRect();
    const mx = r.width / 2;
    const my = r.height / 2;
    const v = viewRef.current;
    const newScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, v.scale * factor));
    const k = newScale / v.scale;
    v.tx = mx - (mx - v.tx) * k;
    v.ty = my - (my - v.ty) * k;
    v.scale = newScale;
    setZoomPct(Math.round(newScale * 100));
  };
  const resetView = () => {
    viewRef.current = { scale: 1, tx: 0, ty: 0 };
    setZoomPct(100);
  };

  // ── Live mode: fetch graph, retry until core responds ───────────────
  useEffect(() => {
    if (!live) return;
    let cancelled = false;
    let retryDelay = 1000;

    const attempt = async () => {
      try {
        const g = await fetchGraph(cortexHttp);
        if (cancelled) return;
        apiGraphRef.current = g;
        setFetchFailed(false);
        setGraphMeta({ nodes: g.nodes.length, edges: g.edges.length });
        setGraphEmpty(g.nodes.length === 0);
        setGraphRevision((v) => v + 1);
      } catch {
        if (cancelled) return;
        setFetchFailed(true);
        retryDelay = Math.min(retryDelay * 1.5, 8000);
        setTimeout(attempt, retryDelay);
      }
    };
    void attempt();
    return () => { cancelled = true; };
  }, [live, cortexHttp]);

  const refreshGraph = async () => {
    const g = await fetchGraph(cortexHttp);
    apiGraphRef.current = g;
    setGraphMeta({ nodes: g.nodes.length, edges: g.edges.length });
    setGraphEmpty(g.nodes.length === 0);
    setGraphRevision((v) => v + 1);
  };

  // ── Live mode: spike WS subscription (with reconnect backoff). ────
  useEffect(() => {
    if (!live) return;
    let stopped = false;
    let ws: WebSocket | null = null;
    let retry = 0;

    const connect = () => {
      if (stopped) return;
      const url = cortexWsUrl(cortexWs);
      setWsState("connecting");
      ws = new WebSocket(url);
      ws.onopen = () => { retry = 0; setWsState("open"); };
      ws.onclose = () => {
        setWsState("closed");
        if (stopped) return;
        retry = Math.min(retry + 1, 8);
        const delay = 250 * Math.pow(1.8, retry);
        setTimeout(connect, delay);
      };
      ws.onerror = () => { /* onclose will handle reconnect */ };
      ws.onmessage = (ev) => {
        try {
          const frame: CortexSpikeFrame = JSON.parse(ev.data);
          (pendingSpikesRef as unknown as { current: (string | number)[] }).current.push(
            ...frame.events.map((e) => e.node_id as unknown as string),
          );
        } catch {}
      };
    };
    connect();

    return () => {
      stopped = true;
      try { ws?.close(); } catch {}
    };
  }, [live, cortexWs]);

  // ── Live mode: weights WS subscription. ───────────────────────────
  useEffect(() => {
    if (!live) return;
    let stopped = false;
    let ws: WebSocket | null = null;
    let retry = 0;

    const connect = () => {
      if (stopped) return;
      const url = cortexWeightsWsUrl(cortexHttp);
      ws = new WebSocket(url);
      ws.onopen = () => { retry = 0; };
      ws.onclose = () => {
        if (stopped) return;
        retry = Math.min(retry + 1, 8);
        setTimeout(connect, 250 * Math.pow(1.8, retry));
      };
      ws.onerror = () => { /* close handler reconnects */ };
      ws.onmessage = (ev) => {
        try {
          const frame: CortexWeightFrame = JSON.parse(ev.data);
          const map = pendingWeightsRef.current;
          for (const d of frame.deltas) map.set(d.edge_id, d.w);
        } catch {}
      };
    };
    connect();

    return () => {
      stopped = true;
      try { ws?.close(); } catch {}
    };
  }, [live, cortexHttp]);

  // ── Click to stimulate ────────────────────────────────────────────
  useEffect(() => {
    if (!live) return;
    const wrap = wrapRef.current;
    const canvas = canvasRef.current;
    if (!wrap || !canvas) return;

    let downX = 0, downY = 0, downT = 0;
    const onDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      downX = e.clientX;
      downY = e.clientY;
      downT = performance.now();
    };
    const onClick = async (e: MouseEvent) => {
      if (buildModeRef.current) return;
      const dx = e.clientX - downX;
      const dy = e.clientY - downY;
      // Real click, not a drag.
      if (Math.hypot(dx, dy) > 4 || performance.now() - downT > 350) return;
      const rect = canvas.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const sy = e.clientY - rect.top;
      // Map screen → world via current view.
      const v = viewRef.current;
      const wx = (sx - v.tx) / v.scale;
      const wy = (sy - v.ty) / v.scale;

      const idx = (canvas as unknown as { _hitTest?: (x: number, y: number) => string | null })
        ._hitTest?.(wx, wy);
      if (!idx) return;
      try {
        await postStimulate(idx, clickStimulusCurrent, clickStimulusDurationMs, cortexHttp);
      } catch (err) {
        console.warn("[connectome] stimulate failed", err);
      }
    };

    wrap.addEventListener("pointerdown", onDown);
    wrap.addEventListener("click", onClick);
    return () => {
      wrap.removeEventListener("pointerdown", onDown);
      wrap.removeEventListener("click", onClick);
    };
  }, [live, cortexHttp, clickStimulusCurrent, clickStimulusDurationMs]);

  // ── Click to inspect params ───────────────────────────────────────
  useEffect(() => {
    if (!live) return;
    const wrap = wrapRef.current;
    const canvas = canvasRef.current;
    if (!wrap || !canvas) return;

    let downX = 0;
    let downY = 0;
    let downT = 0;
    const onDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      downX = e.clientX;
      downY = e.clientY;
      downT = performance.now();
    };
    const onClick = (e: MouseEvent) => {
      if (buildModeRef.current) return;
      const dx = e.clientX - downX;
      const dy = e.clientY - downY;
      if (Math.hypot(dx, dy) > 4 || performance.now() - downT > 350) return;
      const rect = canvas.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const sy = e.clientY - rect.top;
      const v = viewRef.current;
      const wx = (sx - v.tx) / v.scale;
      const wy = (sy - v.ty) / v.scale;
      const hit = (canvas as unknown as { _inspectHit?: (x: number, y: number) => HitTarget | null })
        ._inspectHit?.(wx, wy);
      if (hit) {
        setParamTarget(hit);
      }
    };

    wrap.addEventListener("pointerdown", onDown);
    wrap.addEventListener("click", onClick);
    return () => {
      wrap.removeEventListener("pointerdown", onDown);
      wrap.removeEventListener("click", onClick);
    };
  }, [live]);

  // ── Build mode: add nodes and connect edges using existing graph CRUD. ──
  useEffect(() => {
    if (!live) return;
    const wrap = wrapRef.current;
    const canvas = canvasRef.current;
    if (!wrap || !canvas) return;

    let downX = 0;
    let downY = 0;
    let downWorldX = 0;
    let downWorldY = 0;
    let downNodeId: string | null = null;
    let moved = false;

    const screenToWorld = (clientX: number, clientY: number) => {
      const rect = canvas.getBoundingClientRect();
      const sx = clientX - rect.left;
      const sy = clientY - rect.top;
      const v = viewRef.current;
      return { x: (sx - v.tx) / v.scale, y: (sy - v.ty) / v.scale };
    };

    const hitNode = (x: number, y: number) =>
      (canvas as unknown as { _hitTest?: (x: number, y: number) => string | null })._hitTest?.(x, y) ?? null;

    const onPointerDown = (e: PointerEvent) => {
      if (!buildModeRef.current || e.button !== 0) return;
      const world = screenToWorld(e.clientX, e.clientY);
      downX = e.clientX;
      downY = e.clientY;
      downWorldX = world.x;
      downWorldY = world.y;
      downNodeId = hitNode(world.x, world.y);
      moved = false;
      if (downNodeId) {
        buildDraftRef.current = {
          fromId: downNodeId,
          fromX: world.x,
          fromY: world.y,
          toX: world.x,
          toY: world.y,
        };
      }
      wrap.setPointerCapture(e.pointerId);
      e.preventDefault();
      e.stopPropagation();
    };

    const onPointerMove = (e: PointerEvent) => {
      if (!buildModeRef.current || !downNodeId || !buildDraftRef.current) return;
      const world = screenToWorld(e.clientX, e.clientY);
      moved ||= Math.hypot(e.clientX - downX, e.clientY - downY) > 4;
      buildDraftRef.current = { ...buildDraftRef.current, toX: world.x, toY: world.y };
    };

    const onPointerUp = async (e: PointerEvent) => {
      if (!buildModeRef.current) return;
      try { wrap.releasePointerCapture(e.pointerId); } catch {}
      const world = screenToWorld(e.clientX, e.clientY);
      const upNodeId = hitNode(world.x, world.y);
      const fromNodeId = downNodeId;
      buildDraftRef.current = null;
      e.preventDefault();
      e.stopPropagation();

      try {
        if (fromNodeId && upNodeId && fromNodeId !== upNodeId) {
          setBuildStatus("creating edge...");
          await createGraphEdge({
            pre_id: fromNodeId,
            post_id: upNodeId,
            weight: 0.5,
            edge_type: "manual",
            metadata: { created_by: "connectome-build-mode" },
          }, cortexHttp);
          await refreshGraph();
          setBuildStatus("edge created · drag another connection");
          return;
        }

        if (!fromNodeId && !upNodeId && !moved) {
          const label = `manual-${(graphMeta?.nodes ?? 0) + 1}`;
          setBuildStatus("creating neuron...");
          await createGraphNode({
            label,
            node_type: "manual",
            metadata: {
              x: downWorldX,
              y: downWorldY,
              created_by: "connectome-build-mode",
            },
          }, cortexHttp);
          await refreshGraph();
          setBuildStatus(`created ${label} · drag from it to connect`);
        }
      } catch (err) {
        console.warn("[connectome] build mode mutation failed", err);
        const message = err instanceof Error ? err.message : String(err);
        setBuildStatus(`build failed: ${message}`);
      } finally {
        downNodeId = null;
      }
    };

    wrap.addEventListener("pointerdown", onPointerDown);
    wrap.addEventListener("pointermove", onPointerMove);
    wrap.addEventListener("pointerup", onPointerUp);
    return () => {
      wrap.removeEventListener("pointerdown", onPointerDown);
      wrap.removeEventListener("pointermove", onPointerMove);
      wrap.removeEventListener("pointerup", onPointerUp);
    };
  }, [live, cortexHttp, graphMeta?.nodes]);

  // ── Main animation loop ───────────────────────────────────────────
  useEffect(() => {
    const wrap = wrapRef.current;
    const canvas = canvasRef.current;
    if (!wrap || !canvas) return;

    const ctx = canvas.getContext("2d")!;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);

    const state = {
      graph: null as Graph | null,
      spikes: [] as Spike[],
      lastT: 0,
      spawnAcc: 0,
      spikeWindow: [] as number[],
      lastReport: 0,
      size: { w: 0, h: 0 },
      hoverIdx: null as number | null,
    };

    const rebuild = () => {
      const w = state.size.w;
      const h = state.size.h;
      if (live && apiGraphRef.current) {
        state.graph = buildFromApi(apiGraphRef.current.nodes, apiGraphRef.current.edges, w, h);
      } else {
        state.graph = buildConnectome(nodeCount, w, h);
      }
      state.spikes = [];
    };

    const resize = () => {
      const rect = wrap.getBoundingClientRect();
      const w = Math.max(200, Math.floor(rect.width));
      const h = Math.max(200, Math.floor(rect.height));
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      canvas.style.width = w + "px";
      canvas.style.height = h + "px";
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      state.size = { w, h };
      rebuild();
    };
    resize();

    // Install hit-testers for click handlers.
    (canvas as unknown as { _hitTest?: (x: number, y: number) => string | null })._hitTest =
      (x: number, y: number) => {
        const g = state.graph;
        if (!g) return null;
        let best = -1;
        let bestD2 = Infinity;
        for (const n of g.nodes) {
          const dx = n.x - x;
          const dy = n.y - y;
          const d2 = dx * dx + dy * dy;
          const r = Math.max(10, n.r * 4);
          if (d2 < r * r && d2 < bestD2) { best = n.i; bestD2 = d2; }
        }
        if (best < 0) return null;
        return g.nodes[best].id ?? null;
      };
    (canvas as unknown as { _inspectHit?: (x: number, y: number) => HitTarget | null })._inspectHit =
      (x: number, y: number) => {
        const g = state.graph;
        if (!g) return null;
        let bestNode = -1;
        let bestNodeD2 = Infinity;
        for (const n of g.nodes) {
          const dx = n.x - x;
          const dy = n.y - y;
          const d2 = dx * dx + dy * dy;
          const r = Math.max(10, n.r * 4);
          if (d2 < r * r && d2 < bestNodeD2) { bestNode = n.i; bestNodeD2 = d2; }
        }
        if (bestNode >= 0) {
          const n = g.nodes[bestNode];
          return n.id ? { kind: "node", id: n.id, label: n.label ?? n.id.slice(0, 8) } : null;
        }

        let bestEdge: GraphEdge | null = null;
        let bestEdgeD2 = Infinity;
        for (const e of g.edges) {
          if (!e.id) continue;
          const a = g.nodes[e.a];
          const b = g.nodes[e.b];
          const d2 = edgeDistanceSquared(x, y, a.x, a.y, b.x, b.y);
          if (d2 < bestEdgeD2) {
            bestEdgeD2 = d2;
            bestEdge = e;
          }
        }
        if (bestEdge && bestEdgeD2 < 100 / (viewRef.current.scale ** 2)) {
          const a = g.nodes[bestEdge.a];
          const b = g.nodes[bestEdge.b];
          return {
            kind: "synapse",
            id: bestEdge.id!,
            label: `${a.label ?? a.id?.slice(0, 8) ?? "?"} → ${b.label ?? b.id?.slice(0, 8) ?? "?"}`,
          };
        }
        return null;
      };

    const ro = new ResizeObserver(resize);
    ro.observe(wrap);

    // If live graph arrives after first build, rebuild once it shows up.
    let livePoll: number | null = null;
    if (live) {
      const tryRebuild = () => {
        if (apiGraphRef.current && state.graph && state.graph.idToIndex.size === 0) {
          rebuild();
        }
      };
      livePoll = window.setInterval(tryRebuild, 300);
    }

    const spawnRandomSpike = (now: number) => {
      const g = state.graph;
      if (!g) return;
      const startIdx = Math.floor(Math.random() * g.nodes.length);
      const node = g.nodes[startIdx];
      if (!node.neighbors.length) return;
      const dest = node.neighbors[Math.floor(Math.random() * node.neighbors.length)];
      const dx = g.nodes[dest].x - node.x;
      const dy = g.nodes[dest].y - node.y;
      const len = Math.sqrt(dx * dx + dy * dy);
      const duration = 240 + len * 1.2;
      state.spikes.push({ from: startIdx, to: dest, t0: now, dur: duration });
      state.spikeWindow.push(now);
    };

    const spawnSpikeFromNode = (idx: number, now: number) => {
      const g = state.graph;
      if (!g) return;
      const node = g.nodes[idx];
      if (!node) return;
      node.fire = Math.min(1, node.fire + 0.95);
      if (node.hot) node.hotFire = Math.min(1, node.hotFire + 0.9);
      state.spikeWindow.push(now);
      if (!node.neighbors.length) return;
      const dest = node.neighbors[Math.floor(Math.random() * node.neighbors.length)];
      const dx = g.nodes[dest].x - node.x;
      const dy = g.nodes[dest].y - node.y;
      const len = Math.sqrt(dx * dx + dy * dy);
      const duration = 200 + len * 1.1;
      state.spikes.push({ from: idx, to: dest, t0: now, dur: duration });
    };

    let rafId = 0;

    const step = (now: number) => {
      const { stateKey, colors, wireframe, onSpikeRate, live } = propsRef.current;
      const preset = STATE_PRESETS[stateKey];
      const intensity = preset.intensity;
      const paused = stateKey === "offline";

      if (!state.lastT) state.lastT = now;
      const dt = Math.min(80, now - state.lastT);
      state.lastT = now;

      // Drain WS-driven spikes (live) or run the synthetic ticker (mock).
      if (live) {
        const g = state.graph;
        const queue = pendingSpikesRef.current as unknown as string[];
        if (g && queue.length) {
          // Bound work per frame so a flood doesn't stall the render loop.
          const batch = queue.splice(0, Math.min(queue.length, 200));
          for (const id of batch) {
            const idx = g.idToIndex.get(id);
            if (idx !== undefined) spawnSpikeFromNode(idx, now);
          }
        }
        // Apply pending weight updates to the live graph. We compare
        // against the previous weight to set a transient pulse — that
        // way the user catches the *moment* of STDP, not just the
        // settled steady-state thickness.
        if (g && pendingWeightsRef.current.size) {
          const m = pendingWeightsRef.current;
          for (const [edgeId, w] of m) {
            const ei = g.edgeIdToIndex.get(edgeId);
            if (ei !== undefined) {
              const edge = g.edges[ei];
              const delta = w - edge.weight;
              edge.weight = w;
              // Threshold on |Δw| so floating-point noise doesn't flash
              // the whole graph. Magnitude scales pulse intensity (cap 1).
              if (Math.abs(delta) > 0.005) {
                edge.pulse = Math.min(1, Math.abs(delta) * 8);
                edge.pulseDir = delta > 0 ? 1 : -1;
              }
            }
          }
          m.clear();
        }
      } else if (!paused) {
        const targetSps = 2 + intensity * intensity * 110;
        state.spawnAcc += (targetSps * dt) / 1000;
        while (state.spawnAcc >= 1) {
          spawnRandomSpike(now);
          state.spawnAcc -= 1;
        }
      }

      while (state.spikeWindow.length && now - state.spikeWindow[0] > 1000) {
        state.spikeWindow.shift();
      }
      if (onSpikeRate && Math.floor(now / 250) !== Math.floor(state.lastReport / 250)) {
        onSpikeRate(state.spikeWindow.length);
        state.lastReport = now;
      }

      const g = state.graph;
      const { w, h } = state.size;
      if (!g) {
        rafId = requestAnimationFrame(step);
        return;
      }

      for (const n of g.nodes) {
        n.fire *= Math.pow(0.001, dt / 1000);
        n.hotFire *= Math.pow(0.0005, dt / 1000);
        if (n.fire < 0.001) n.fire = 0;
        if (n.hotFire < 0.001) n.hotFire = 0;
      }
      // STDP pulses decay on a faster constant (~250ms half-life) — the
      // intent is a brief flash, not a sustained highlight.
      const pulseFalloff = Math.pow(0.06, dt / 1000);
      for (const e of g.edges) {
        if (e.pulse > 0) {
          e.pulse *= pulseFalloff;
          if (e.pulse < 0.01) {
            e.pulse = 0;
            e.pulseDir = 0;
          }
        }
      }

      const next: Spike[] = [];
      for (const sp of state.spikes) {
        const p = (now - sp.t0) / sp.dur;
        if (p >= 1) {
          const dst = g.nodes[sp.to];
          dst.fire = Math.min(1, dst.fire + 0.9);
          if (dst.hot) dst.hotFire = Math.min(1, dst.hotFire + 0.85);
          // In live mode we don't fake a cascade; real cascades arrive
          // as separate spike events. Mock keeps the existing behavior.
          if (!live && !paused) {
            const cascadeP = 0.18 + intensity * 0.55;
            if (Math.random() < cascadeP && dst.neighbors.length) {
              let nbr = dst.neighbors[Math.floor(Math.random() * dst.neighbors.length)];
              if (nbr === sp.from && dst.neighbors.length > 1 && Math.random() < 0.6) {
                nbr = dst.neighbors[Math.floor(Math.random() * dst.neighbors.length)];
              }
              const a = dst;
              const b = g.nodes[nbr];
              const dx = b.x - a.x;
              const dy = b.y - a.y;
              const len = Math.sqrt(dx * dx + dy * dy);
              next.push({ from: sp.to, to: nbr, t0: now, dur: 240 + len * 1.2 });
              state.spikeWindow.push(now);
            }
          }
          continue;
        }
        next.push(sp);
      }
      state.spikes = next.slice(-1600);

      // ── Render ──────────────────────────────────────────────────────
      ctx.clearRect(0, 0, w, h);
      ctx.fillStyle = colors.bg;
      ctx.fillRect(0, 0, w, h);

      const view = viewRef.current;

      ctx.save();
      ctx.translate(view.tx, view.ty);
      ctx.scale(view.scale, view.scale);

      const baseStep = 80;
      let gridStep = baseStep;
      while (gridStep * view.scale < 40) gridStep *= 2;
      while (gridStep * view.scale > 160) gridStep /= 2;

      const wx0 = -view.tx / view.scale;
      const wy0 = -view.ty / view.scale;
      const wx1 = wx0 + w / view.scale;
      const wy1 = wy0 + h / view.scale;
      const gx0 = Math.floor(wx0 / gridStep) * gridStep;
      const gy0 = Math.floor(wy0 / gridStep) * gridStep;

      ctx.strokeStyle = colors.gridLine;
      ctx.lineWidth = 1 / view.scale;
      ctx.beginPath();
      for (let x = gx0; x <= wx1; x += gridStep) {
        ctx.moveTo(x, wy0);
        ctx.lineTo(x, wy1);
      }
      for (let y = gy0; y <= wy1; y += gridStep) {
        ctx.moveTo(wx0, y);
        ctx.lineTo(wx1, y);
      }
      ctx.stroke();

      if (wireframe) {
        ctx.strokeStyle = colors.edge;
        ctx.lineWidth = 1 / view.scale;
        ctx.beginPath();
        for (const e of g.edges) {
          const a = g.nodes[e.a];
          const b = g.nodes[e.b];
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
        }
        ctx.stroke();
      } else {
        for (const e of g.edges) {
          const a = g.nodes[e.a];
          const b = g.nodes[e.b];
          const act = Math.max(a.fire, b.fire);
          // weight ∈ [0,1]; centered at 0.5 → 1.0 thickness, scaling linearly
          // 0 → 0.25× thickness, 1 → 2.5× thickness. Activity layered on top.
          // STDP pulse momentarily fattens the edge so the moment of
          // plasticity reads as a brief bloom on top of the steady weight.
          const w = e.weight;
          const thickness = (0.25 + w * 2.25 + e.pulse * 0.9) / view.scale;
          ctx.lineWidth = thickness;
          if (e.longRange) {
            // Slightly pink-tinged for high-weight long-range edges so the
            // "this connection has been potentiated" reading is glanceable.
            const tintBase = 0.18 + act * 0.45 + Math.max(0, w - 0.5) * 0.5;
            ctx.strokeStyle = `rgba(180,220,255,${Math.min(1, tintBase)})`;
          } else if (w > 0.55) {
            // Potentiated: brighter cyan.
            const aBase = 0.18 + (w - 0.55) * 1.5 + act * 0.35;
            ctx.strokeStyle = `rgba(125,249,255,${Math.min(1, aBase)})`;
          } else if (w < 0.45) {
            // Depressed: dimmer.
            const aBase = 0.04 + w * 0.18 + act * 0.3;
            ctx.strokeStyle = `rgba(125,249,255,${Math.min(1, aBase)})`;
          } else {
            ctx.strokeStyle =
              act > 0.05 ? `rgba(125,249,255,${0.12 + act * 0.35})` : colors.edge;
          }
          ctx.beginPath();
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
          ctx.stroke();
          // STDP pulse overlay: green-ish for potentiation, amber for
          // depression. Drawn over the steady-state stroke so the
          // settled weight color reads through as the pulse fades.
          if (e.pulse > 0) {
            const alpha = Math.min(0.9, e.pulse);
            ctx.strokeStyle =
              e.pulseDir > 0
                ? `rgba(125,255,180,${alpha})`
                : `rgba(255,170,90,${alpha})`;
            ctx.lineWidth = (0.7 + e.pulse * 1.6) / view.scale;
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(b.x, b.y);
            ctx.stroke();
          }
        }
      }

      const draft = buildDraftRef.current;
      if (draft) {
        ctx.strokeStyle = "rgba(255,255,255,0.55)";
        ctx.lineWidth = 1.2 / view.scale;
        ctx.setLineDash([7 / view.scale, 7 / view.scale]);
        ctx.beginPath();
        ctx.moveTo(draft.fromX, draft.fromY);
        ctx.lineTo(draft.toX, draft.toY);
        ctx.stroke();
        ctx.setLineDash([]);
      }

      if (!wireframe) {
        for (const sp of state.spikes) {
          const p = Math.min(1, (now - sp.t0) / sp.dur);
          const a = g.nodes[sp.from];
          const b = g.nodes[sp.to];
          const x = a.x + (b.x - a.x) * p;
          const y = a.y + (b.y - a.y) * p;
          const tx = a.x + (b.x - a.x) * Math.max(0, p - 0.12);
          const ty = a.y + (b.y - a.y) * Math.max(0, p - 0.12);
          const grad = ctx.createLinearGradient(tx, ty, x, y);
          grad.addColorStop(0, "rgba(125,249,255,0)");
          grad.addColorStop(1, "rgba(125,249,255,0.95)");
          ctx.strokeStyle = grad;
          ctx.lineWidth = 1.6 / view.scale;
          ctx.beginPath();
          ctx.moveTo(tx, ty);
          ctx.lineTo(x, y);
          ctx.stroke();
          ctx.fillStyle = colors.cyan;
          ctx.beginPath();
          ctx.arc(x, y, 1.8 / view.scale, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      for (const n of g.nodes) {
        if (wireframe) {
          ctx.strokeStyle = colors.edgeStrong;
          ctx.lineWidth = 1 / view.scale;
          ctx.beginPath();
          ctx.arc(n.x, n.y, n.r + 0.5, 0, Math.PI * 2);
          ctx.stroke();
          continue;
        }
        if (n.fire > 0.02 || n.hotFire > 0.02) {
          const useHot = n.hotFire > 0.25;
          const color = useHot ? colors.pink : colors.cyan;
          const glowR = (useHot ? 18 : 12) * (useHot ? n.hotFire : n.fire) + 4;
          const g1 = ctx.createRadialGradient(n.x, n.y, 0, n.x, n.y, glowR);
          g1.addColorStop(
            0,
            useHot
              ? `rgba(255,93,143,${0.6 * n.hotFire})`
              : `rgba(125,249,255,${0.55 * n.fire})`,
          );
          g1.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = g1;
          ctx.beginPath();
          ctx.arc(n.x, n.y, glowR, 0, Math.PI * 2);
          ctx.fill();
          ctx.fillStyle = color;
          ctx.beginPath();
          ctx.arc(n.x, n.y, n.r + 0.6, 0, Math.PI * 2);
          ctx.fill();
        } else {
          ctx.fillStyle = colors.nodeIdle;
          ctx.beginPath();
          ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      ctx.restore();

      rafId = requestAnimationFrame(step);
    };
    rafId = requestAnimationFrame(step);

    // Hover label tracking — separate from the click handler so it
    // works in both live and mock modes.
    const onMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const sx = e.clientX - rect.left;
      const sy = e.clientY - rect.top;
      const v = viewRef.current;
      const wx = (sx - v.tx) / v.scale;
      const wy = (sy - v.ty) / v.scale;
      const g = state.graph;
      if (!g) return;
      let best = -1;
      let bestD2 = Infinity;
      for (const n of g.nodes) {
        const dx = n.x - wx;
        const dy = n.y - wy;
        const d2 = dx * dx + dy * dy;
        const r = Math.max(10, n.r * 4);
        if (d2 < r * r && d2 < bestD2) { best = n.i; bestD2 = d2; }
      }
      if (best >= 0) {
        const lbl = g.nodes[best].label ?? null;
        if (lbl !== state.hoverIdx as unknown as string | null) {
          state.hoverIdx = best as unknown as number;
          setHoverLabel(lbl);
        }
      } else if (state.hoverIdx !== null) {
        state.hoverIdx = null;
        setHoverLabel(null);
      }
    };
    wrap.addEventListener("mousemove", onMove);

    return () => {
      ro.disconnect();
      cancelAnimationFrame(rafId);
      if (livePoll) clearInterval(livePoll);
      wrap.removeEventListener("mousemove", onMove);
    };
  }, [nodeCount, live, graphRevision]);

  const preset = STATE_PRESETS[stateKey];
  const overlayLabel = live
    ? wsState === "open"
      ? `live · ws://core/spikes · ${graphMeta?.nodes ?? "?"} nodes`
      : `live · connecting…`
    : "connectome · placeholder · awaiting ws://core/spikes";
  const buildModeEnabled = live && buildMode;

  return (
    <main className="panel connectome">
      <div className="connectome-canvas-wrap">
        <div
          ref={wrapRef}
          style={{
            position: "absolute",
            inset: 0,
            cursor: panning ? "grabbing" : buildModeEnabled ? "copy" : live ? "crosshair" : "default",
            touchAction: "none",
            overscrollBehavior: "contain",
          }}
        >
          <canvas
            ref={canvasRef}
            style={{ display: "block", width: "100%", height: "100%" }}
          />
        </div>
        {fetchFailed && live && (
          <div
            style={{
              position: "absolute",
              top: "50%",
              left: "50%",
              transform: "translate(-50%, -50%)",
              background: "rgba(10,13,18,0.88)",
              border: "1px solid rgba(255,93,143,0.4)",
              borderRadius: 6,
              padding: "14px 20px",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
              fontSize: 12,
              color: "rgba(255,93,143,0.9)",
              pointerEvents: "none",
              zIndex: 10,
              textAlign: "center",
              lineHeight: 1.6,
            }}
          >
            core unreachable · {cortexHttpBase(cortexHttp)}<br />
            <span style={{ color: "rgba(125,249,255,0.5)" }}>
              check that the desktop supervisor started · mock graph shown
            </span>
          </div>
        )}
        {graphEmpty && live && !fetchFailed && (
          <div
            style={{
              position: "absolute",
              top: "50%",
              left: "50%",
              transform: "translate(-50%, -50%)",
              background: "rgba(10,13,18,0.92)",
              border: "1px solid rgba(125,249,255,0.22)",
              borderRadius: 8,
              padding: "24px 28px",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
              fontSize: 12,
              color: "rgba(229,231,235,0.8)",
              zIndex: 10,
              textAlign: "center",
              lineHeight: 1.7,
              maxWidth: 380,
              pointerEvents: "auto",
            }}
          >
            <div style={{ fontSize: 11, color: "rgba(125,249,255,0.6)", letterSpacing: "0.12em", textTransform: "uppercase", marginBottom: 10 }}>
              empty network · 0 nodes · 0 edges
            </div>
            <div style={{ marginBottom: 16, fontSize: 12, color: "rgba(229,231,235,0.6)", lineHeight: 1.6 }}>
              This cortex folder has no topology yet. Use{" "}
              <span style={{ color: "rgba(125,249,255,0.9)" }}>build mode</span>{" "}
              to add neurons manually, or go back to the start screen to seed a starter network.
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "center", flexWrap: "wrap" }}>
              <button
                className="zoom-btn"
                style={{
                  padding: "7px 14px",
                  border: "1px solid rgba(125,249,255,0.3)",
                  color: "rgba(125,249,255,0.9)",
                  background: "rgba(125,249,255,0.06)",
                  borderRadius: 4,
                  fontSize: 11,
                  cursor: "pointer",
                  pointerEvents: "auto",
                }}
                onClick={() => {
                  setBuildMode(true);
                  setBuildStatus("click empty space to add · drag node to node to connect");
                }}
              >
                enable build mode
              </button>
              {onReopen && (
                <button
                  className="zoom-btn"
                  style={{
                    padding: "7px 14px",
                    border: "1px solid rgba(125,249,255,0.15)",
                    color: "rgba(229,231,235,0.6)",
                    background: "transparent",
                    borderRadius: 4,
                    fontSize: 11,
                    cursor: "pointer",
                    pointerEvents: "auto",
                  }}
                  onClick={onReopen}
                >
                  back to start screen
                </button>
              )}
            </div>
          </div>
        )}
        {live && (
          <aside
            style={{
              position: "absolute",
              top: 58,
              right: 18,
              width: 320,
              maxHeight: "calc(100% - 120px)",
              overflow: "hidden",
              background: "linear-gradient(180deg, rgba(13,17,23,0.94), rgba(10,13,18,0.88))",
              border: "1px solid rgba(125,249,255,0.18)",
              borderRadius: 10,
              boxShadow: "0 18px 70px rgba(0,0,0,0.45)",
              color: "rgba(229,231,235,0.9)",
              zIndex: 9,
              pointerEvents: "auto",
            }}
          >
            <div
              className="mono"
              style={{
                display: "flex",
                justifyContent: "space-between",
                gap: 12,
                padding: "12px 14px",
                borderBottom: "1px solid rgba(125,249,255,0.12)",
                fontSize: 11,
                letterSpacing: "0.08em",
                textTransform: "uppercase",
              }}
            >
              <span>{paramTarget ? `${paramTarget.kind} params` : "parameter panel"}</span>
              <button
                className="zoom-btn"
                onClick={() => setParamTarget(null)}
                title="Clear selection"
              >
                clear
              </button>
            </div>
            <div style={{ padding: 14, overflowY: "auto", maxHeight: "calc(100vh - 240px)" }}>
              {paramTarget ? (
                <>
                  <div style={{ marginBottom: 10 }}>
                    <div style={{ fontSize: 13, fontWeight: 650 }}>
                      {paramTarget.label ?? paramTarget.id.slice(0, 8)}
                    </div>
                    <div className="mono" style={{ fontSize: 10, color: "rgba(125,249,255,0.55)", marginTop: 3 }}>
                      {paramTarget.id}
                    </div>
                    <div className="mono" style={{ fontSize: 10, color: paramStatus.startsWith("save failed") || paramStatus.startsWith("load failed") ? "rgba(255,93,143,0.9)" : "rgba(255,255,255,0.45)", marginTop: 8 }}>
                      {paramStatus}
                    </div>
                  </div>
                  {paramValues ? (
                    <div style={{ display: "grid", gap: 8 }}>
                      {Object.entries(paramValues).map(([key, value]) => {
                        const editable = isParamEditable(key, value);
                        return (
                          <label
                            key={key}
                            style={{
                              display: "grid",
                              gridTemplateColumns: "112px 1fr auto",
                              alignItems: "center",
                              gap: 8,
                              fontSize: 11,
                            }}
                          >
                            <span className="mono" style={{ color: editable ? "rgba(229,231,235,0.78)" : "rgba(229,231,235,0.34)" }}>
                              {key}
                            </span>
                            {typeof value === "boolean" ? (
                              <select
                                disabled={!editable}
                                value={paramDrafts[key] ?? String(value)}
                                onChange={(e) => setParamDrafts((drafts) => ({ ...drafts, [key]: e.target.value }))}
                                style={{
                                  minWidth: 0,
                                  background: "rgba(3,7,18,0.8)",
                                  border: "1px solid rgba(125,249,255,0.18)",
                                  borderRadius: 6,
                                  color: "rgba(229,231,235,0.9)",
                                  padding: "6px 7px",
                                  fontSize: 11,
                                }}
                              >
                                <option value="true">true</option>
                                <option value="false">false</option>
                              </select>
                            ) : (
                              <input
                                disabled={!editable}
                                value={paramDrafts[key] ?? formatParamValue(value)}
                                onChange={(e) => setParamDrafts((drafts) => ({ ...drafts, [key]: e.target.value }))}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter" && editable) void commitParam(key);
                                }}
                                style={{
                                  minWidth: 0,
                                  background: editable ? "rgba(3,7,18,0.8)" : "rgba(3,7,18,0.35)",
                                  border: "1px solid rgba(125,249,255,0.18)",
                                  borderRadius: 6,
                                  color: editable ? "rgba(229,231,235,0.9)" : "rgba(229,231,235,0.42)",
                                  padding: "6px 7px",
                                  fontSize: 11,
                                  fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
                                }}
                              />
                            )}
                            <button
                              className="zoom-btn"
                              disabled={!editable || (paramDrafts[key] ?? formatParamValue(value)) === formatParamValue(value)}
                              onClick={() => void commitParam(key)}
                              title={editable ? "Apply parameter" : "Read-only parameter"}
                            >
                              set
                            </button>
                          </label>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="mono" style={{ color: "rgba(255,255,255,0.42)", fontSize: 11 }}>
                      no parameters loaded
                    </div>
                  )}
                  {paramTarget.kind === "node" && (
                    <div style={{ marginTop: 16 }}>
                      <div
                        className="mono"
                        style={{
                          display: "flex",
                          justifyContent: "space-between",
                          fontSize: 10,
                          textTransform: "uppercase",
                          color: "rgba(125,249,255,0.7)",
                          marginBottom: 6,
                        }}
                      >
                        <span>V(t) membrane</span>
                        <span style={{ color: "rgba(255,255,255,0.45)" }}>{voltageStatus}</span>
                      </div>
                      <canvas
                        ref={voltageCanvasRef}
                        width={260}
                        height={96}
                        style={{
                          width: "100%",
                          height: 96,
                          background: "rgba(3,7,18,0.8)",
                          border: "1px solid rgba(125,249,255,0.18)",
                          borderRadius: 6,
                        }}
                      />
                    </div>
                  )}
                </>
              ) : (
                <div className="mono" style={{ color: "rgba(255,255,255,0.42)", fontSize: 11, lineHeight: 1.6 }}>
                  click a neuron or synapse to inspect live parameters. Enter commits a field; `set` applies one field.
                </div>
              )}
            </div>
          </aside>
        )}
        <div className="connectome-overlay">
          <div className="overlay-top">
            <div className="overlay-label mono">
              {overlayLabel}
              {buildModeEnabled && (
                <span style={{ marginLeft: 12, color: "rgba(125,249,255,0.9)" }}>
                  build · {buildStatus}
                </span>
              )}
              {hoverLabel && (
                <span style={{ marginLeft: 12, color: "rgba(255,255,255,0.7)" }}>
                  ▸ {hoverLabel}
                </span>
              )}
            </div>
            <div className="overlay-legend mono">
              <span><i className="dot dot-cyan" /> firing</span>
              <span><i className="dot dot-pink" /> hot</span>
              <span><i className="dot dot-edge" /> edge</span>
            </div>
          </div>
          <div className="overlay-corners">
            <span className="tick tl" />
            <span className="tick tr" />
            <span className="tick bl" />
            <span className="tick br" />
          </div>
          <div className="overlay-bottom">
            <div className="overlay-readout mono">
              <span>nodes <b>{graphMeta?.nodes ?? nodeCount}</b></span>
              <span>edges <b>{graphMeta?.edges ?? Math.round(nodeCount * 3.4)}</b></span>
              <span>i <b>{preset.intensity.toFixed(2)}</b></span>
              <span>zoom <b>{zoomPct}%</b></span>
            </div>
            <div className="overlay-controls mono">
              <button className="zoom-btn" onClick={() => zoomCenter(1 / 1.25)} title="Zoom out">−</button>
              <button className="zoom-btn" onClick={resetView} title="Reset zoom">{zoomPct}%</button>
              <button className="zoom-btn" onClick={() => zoomCenter(1.25)} title="Zoom in">+</button>
              <span className="zoom-sep" />
              <button className="zoom-btn" title="Hold space + drag, or middle-click to pan">pan</button>
              <button className="zoom-btn" onClick={resetView} title="Fit / reset view">fit</button>
              {live && (
                <>
                  <span className="zoom-sep" />
                  <button
                    className="zoom-btn"
                    onClick={() => {
                      setBuildMode((value) => {
                        const next = !value;
                        setBuildStatus(
                          next
                            ? "click empty space to add · drag node to node to connect"
                            : "build mode off",
                        );
                        return next;
                      });
                    }}
                    title="Toggle interactive build mode"
                  >
                    {buildModeEnabled ? "build:on" : "build"}
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      </div>
    </main>
  );
}
