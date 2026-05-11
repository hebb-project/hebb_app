"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { STATE_PRESETS, type Palette, type StateKey } from "@/lib/state";

type Node = {
  i: number;
  x: number;
  y: number;
  r: number;
  hot: boolean;
  fire: number;
  hotFire: number;
  neighbors: number[];
};

type Edge = { a: number; b: number; len: number; longRange?: boolean };

type Graph = { nodes: Node[]; edges: Edge[] };

type Spike = { from: number; to: number; t0: number; dur: number };

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

  const nodes: Node[] = [];
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

  const edges: Edge[] = [];
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
      edges.push({ a: i, b: j, len: Math.sqrt(dists[t].d) });
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
    edges.push({ a: i, b: j, len: Math.sqrt(dx * dx + dy * dy), longRange: true });
    nodes[i].neighbors.push(j);
    nodes[j].neighbors.push(i);
  }

  return { nodes, edges };
}

type Props = {
  stateKey: StateKey;
  nodeCount: number;
  palette: Palette;
  wireframe?: boolean;
  onSpikeRate?: (sps: number) => void;
};

export function ConnectomeView({
  stateKey,
  nodeCount,
  palette,
  wireframe = false,
  onSpikeRate,
}: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const viewRef = useRef<View>({ scale: 1, tx: 0, ty: 0 });
  const [zoomPct, setZoomPct] = useState(100);
  const [panning, setPanning] = useState(false);

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
    [palette]
  );

  const propsRef = useRef({ stateKey, colors, wireframe, onSpikeRate });
  useEffect(() => {
    propsRef.current = { stateKey, colors, wireframe, onSpikeRate };
  });

  // Native wheel listener so we can preventDefault (React's onWheel is passive).
  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;

    const clampScale = (s: number) => Math.max(MIN_SCALE, Math.min(MAX_SCALE, s));

    const zoomAt = (mx: number, my: number, factor: number) => {
      const v = viewRef.current;
      const newScale = clampScale(v.scale * factor);
      const k = newScale / v.scale;
      // Keep the world point under the cursor anchored.
      v.tx = mx - (mx - v.tx) * k;
      v.ty = my - (my - v.ty) * k;
      v.scale = newScale;
      setZoomPct(Math.round(newScale * 100));
    };

    const onWheel = (e: WheelEvent) => {
      // Always prevent the page from scrolling/zooming behind us.
      e.preventDefault();
      const rect = wrap.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      // macOS trackpad pinch sets ctrlKey; ctrl/cmd + scroll also zooms.
      if (e.ctrlKey || e.metaKey) {
        const factor = Math.exp(-e.deltaY * 0.01);
        zoomAt(mx, my, factor);
      } else {
        // Two-finger scroll = pan (Figma-style).
        const v = viewRef.current;
        v.tx -= e.deltaX;
        v.ty -= e.deltaY;
      }
    };

    // Click-and-drag with space or middle mouse to pan as a fallback.
    let dragging = false;
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
    };
    const onPointerUp = (e: PointerEvent) => {
      if (dragging) {
        dragging = false;
        try {
          wrap.releasePointerCapture(e.pointerId);
        } catch {}
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

  // Zoom buttons (anchored to viewport center).
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
      state.graph = buildConnectome(nodeCount, w, h);
      state.spikes = [];
    };
    resize();

    const ro = new ResizeObserver(resize);
    ro.observe(wrap);

    const spawnSpike = (now: number) => {
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

    let rafId = 0;

    const step = (now: number) => {
      const { stateKey, colors, wireframe, onSpikeRate } = propsRef.current;
      const preset = STATE_PRESETS[stateKey];
      const intensity = preset.intensity;
      const paused = stateKey === "offline";

      if (!state.lastT) state.lastT = now;
      const dt = Math.min(80, now - state.lastT);
      state.lastT = now;

      if (!paused) {
        const targetSps = 2 + intensity * intensity * 110;
        state.spawnAcc += (targetSps * dt) / 1000;
        while (state.spawnAcc >= 1) {
          spawnSpike(now);
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

      const next: Spike[] = [];
      for (const sp of state.spikes) {
        const p = (now - sp.t0) / sp.dur;
        if (p >= 1) {
          const dst = g.nodes[sp.to];
          dst.fire = Math.min(1, dst.fire + 0.9);
          if (dst.hot) dst.hotFire = Math.min(1, dst.hotFire + 0.85);
          const cascadeP = 0.18 + intensity * 0.55;
          if (!paused && Math.random() < cascadeP && dst.neighbors.length) {
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
          continue;
        }
        next.push(sp);
      }
      state.spikes = next.slice(-1200);

      // ── Render ────────────────────────────────────────────────────────
      ctx.clearRect(0, 0, w, h);
      ctx.fillStyle = colors.bg;
      ctx.fillRect(0, 0, w, h);

      const view = viewRef.current;

      // Grid in world space — moves and scales with content (Figma-like).
      // Step adapts so the grid never gets too dense/sparse on the screen.
      ctx.save();
      ctx.translate(view.tx, view.ty);
      ctx.scale(view.scale, view.scale);

      const baseStep = 80;
      let gridStep = baseStep;
      while (gridStep * view.scale < 40) gridStep *= 2;
      while (gridStep * view.scale > 160) gridStep /= 2;

      // Visible world rect.
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

      // Edges.
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
        ctx.lineWidth = 1 / view.scale;
        for (const e of g.edges) {
          const a = g.nodes[e.a];
          const b = g.nodes[e.b];
          const act = Math.max(a.fire, b.fire);
          if (e.longRange) {
            ctx.strokeStyle =
              act > 0.05 ? `rgba(125,249,255,${0.18 + act * 0.45})` : colors.edgeStrong;
          } else {
            ctx.strokeStyle =
              act > 0.05 ? `rgba(125,249,255,${0.12 + act * 0.35})` : colors.edge;
          }
          ctx.beginPath();
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
          ctx.stroke();
        }
      }

      // Spikes.
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

      // Nodes.
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
              : `rgba(125,249,255,${0.55 * n.fire})`
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

    return () => {
      ro.disconnect();
      cancelAnimationFrame(rafId);
    };
  }, [nodeCount]);

  const preset = STATE_PRESETS[stateKey];

  return (
    <main className="panel connectome">
      <div className="connectome-canvas-wrap">
        <div
          ref={wrapRef}
          style={{
            position: "absolute",
            inset: 0,
            cursor: panning ? "grabbing" : "default",
            touchAction: "none",
            overscrollBehavior: "contain",
          }}
        >
          <canvas
            ref={canvasRef}
            style={{ display: "block", width: "100%", height: "100%" }}
          />
        </div>
        <div className="connectome-overlay">
          <div className="overlay-top">
            <div className="overlay-label mono">
              connectome · placeholder · awaiting <span className="hl">ws://core/spikes</span>
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
              <span>nodes <b>{nodeCount}</b></span>
              <span>edges <b>~{Math.round(nodeCount * 3.4)}</b></span>
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
            </div>
          </div>
        </div>
      </div>
    </main>
  );
}
