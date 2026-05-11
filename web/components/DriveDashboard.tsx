"use client";

import { useEffect, useState } from "react";
import { STATE_PRESETS, fmt3, type Palette, type StateKey } from "@/lib/state";

type Props = {
  width: number;
  stateKey: StateKey;
  palette: Palette;
};

export function DriveDashboard({ width, stateKey, palette }: Props) {
  const preset = STATE_PRESETS[stateKey];

  const [vals, setVals] = useState({
    ...preset.drives,
    freeEnergy: preset.freeEnergy,
  });

  useEffect(() => {
    let raf = 0;
    const tick = () => {
      setVals((cur) => {
        const ease = (a: number, b: number) => a + (b - a) * 0.08;
        return {
          curiosity: ease(cur.curiosity, preset.drives.curiosity),
          homeostasis: ease(cur.homeostasis, preset.drives.homeostasis),
          social: ease(cur.social, preset.drives.social),
          rest: ease(cur.rest, preset.drives.rest),
          freeEnergy: ease(cur.freeEnergy, preset.freeEnergy),
        };
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [stateKey, preset.drives.curiosity, preset.drives.homeostasis, preset.drives.social, preset.drives.rest, preset.freeEnergy]);

  return (
    <aside className="panel drives" style={{ width }}>
      {/* FREE ENERGY */}
      <section className="block">
        <div className="panel-hd mono">
          <span>free energy</span>
          <span className="panel-hd-meta">F = ⟨log q − log p⟩</span>
        </div>
        <div className="fe-row">
          <div className="fe-number mono tabnum">{fmt3(vals.freeEnergy)}</div>
          <div className="fe-delta mono">
            <span className={vals.freeEnergy > 0.5 ? "delta-up" : "delta-down"}>
              {vals.freeEnergy > 0.5 ? "▲" : "▼"} {fmt3(Math.abs(vals.freeEnergy - 0.5))}
            </span>
            <div className="fe-sub mono">vs baseline 0.500</div>
          </div>
        </div>
        <Bar value={vals.freeEnergy} color={palette.pink} />
        <div className="fe-sparkline mono">
          <Sparkline value={vals.freeEnergy} color={palette.pink} />
        </div>
      </section>

      {/* DRIVES */}
      <section className="block">
        <div className="panel-hd mono">
          <span>intrinsic drives</span>
          <span className="panel-hd-meta">4 active</span>
        </div>
        <DriveRow name="curiosity" value={vals.curiosity} color={palette.purple} note="novelty seeking" />
        <DriveRow name="homeostasis" value={vals.homeostasis} color={palette.cyan} note="internal balance" />
        <DriveRow name="social" value={vals.social} color={palette.pink} note="connection" />
        <DriveRow name="rest" value={vals.rest} color="#7d8590" note="recovery" />
      </section>

      {/* MEMORY */}
      <section className="block">
        <div className="panel-hd mono">
          <span>memory · consolidation</span>
          <span className="panel-hd-meta">replay buffer</span>
        </div>
        <div className="memory-box mono">
          <div className="memory-icon">
            <span className="mem-ring" />
            <span className="mem-ring" />
            <span className="mem-ring" />
          </div>
          <div className="memory-text">replay events</div>
          <div className="memory-sub">awaiting core</div>
        </div>
        <div className="memory-stats mono">
          <div className="ms-row"><span>buffer</span><span className="tabnum">0 / 4096</span></div>
          <div className="ms-row"><span>last replay</span><span className="tabnum">--:--:--</span></div>
          <div className="ms-row"><span>consolidations</span><span className="tabnum">0</span></div>
        </div>
      </section>
    </aside>
  );
}

function Bar({ value, color, height = 4 }: { value: number; color: string; height?: number }) {
  return (
    <div className="bar" style={{ height }}>
      <div className="bar-track" />
      <div
        className="bar-fill"
        style={{
          width: `${Math.max(0, Math.min(1, value)) * 100}%`,
          background: color,
          boxShadow: `0 0 8px ${color}66`,
        }}
      />
      <div className="bar-ticks">
        {[0.25, 0.5, 0.75].map((t) => (
          <span key={t} className="bar-tick" style={{ left: `${t * 100}%` }} />
        ))}
      </div>
    </div>
  );
}

function DriveRow({ name, value, color, note }: { name: string; value: number; color: string; note: string }) {
  return (
    <div className="drive-row">
      <div className="drive-row-top mono">
        <span className="drive-name">{name}</span>
        <span className="drive-value tabnum">{fmt3(value)}</span>
      </div>
      <Bar value={value} color={color} />
      <div className="drive-note mono">{note}</div>
    </div>
  );
}

function Sparkline({ value, color }: { value: number; color: string }) {
  const [hist, setHist] = useState<number[]>(() => Array(40).fill(0.4));
  useEffect(() => {
    const id = setInterval(() => {
      setHist((h) => [...h.slice(1), value + (Math.random() - 0.5) * 0.04]);
    }, 250);
    return () => clearInterval(id);
  }, [value]);

  const w = 320;
  const h = 28;
  const pts = hist
    .map((v, i) => {
      const x = (i / (hist.length - 1)) * w;
      const y = h - v * h;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" style={{ width: "100%", height: h }}>
      <polyline points={pts} fill="none" stroke={color} strokeWidth="1.2" opacity="0.85" />
      <line x1="0" x2={w} y1={h - 0.5} y2={h - 0.5} stroke="#1f2933" strokeWidth="1" />
    </svg>
  );
}
