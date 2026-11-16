"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import type { PhysNode } from "@/lib/types";
import { fmtInt, fmtMs } from "@/lib/format";

const W = 188;
const H = 60;
const GX = 14;
const GY = 30;
const PAD = 6;
/** How long a replay takes on screen, whatever the real execution time. */
const REPLAY_MS = 2600;

type Placed = { n: PhysNode; x: number; y: number; parent?: Placed };

/** Staircase layout, as in database textbooks: the first input sits directly
 *  under its operator, later inputs step to the right of the subtrees before them.
 *  Left-deep join trees stay narrow. */
function layout(root: PhysNode) {
  const placed: Placed[] = [];
  let maxCol = 0;
  let depthMax = 0;
  // next free column at each depth, so subtrees never overlap
  const nextFree: number[] = [];
  const place = (n: PhysNode, desired: number, depth: number, parent?: Placed) => {
    const col = Math.max(desired, nextFree[depth] ?? 0);
    nextFree[depth] = col + 1;
    const p: Placed = { n, x: col * (W + GX), y: depth * (H + GY), parent };
    placed.push(p);
    maxCol = Math.max(maxCol, col);
    depthMax = Math.max(depthMax, depth);
    n.children.forEach((c, i) => place(c, i === 0 ? col : col + i, depth + 1, p));
  };
  place(root, 0, 0);
  return { placed, width: (maxCol + 1) * (W + GX) - GX + PAD * 2, height: (depthMax + 1) * (H + GY) - GY + PAD * 2 };
}

function clip(s: string, n: number) {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

export default function PlanTree({ root, trace, runKey }: { root: PhysNode; trace: [number, number, number][]; runKey: number }) {
  const { placed, width, height } = useMemo(() => layout(root), [root]);
  const t0 = trace.length ? trace[0][0] : 0;
  const t1 = trace.length ? trace[trace.length - 1][0] : 0;
  const span = Math.max(t1 - t0, 0.001);
  // replay position, 0..1
  const [pos, setPos] = useState(1);
  const [playing, setPlaying] = useState(false);
  const raf = useRef<number | null>(null);

  const play = () => {
    if (raf.current) cancelAnimationFrame(raf.current);
    const reduce = typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce || !trace.length) {
      setPos(1);
      return;
    }
    const start = performance.now();
    setPlaying(true);
    const step = (now: number) => {
      const f = Math.min(1, (now - start) / REPLAY_MS);
      setPos(f);
      if (f < 1) raf.current = requestAnimationFrame(step);
      else setPlaying(false);
    };
    raf.current = requestAnimationFrame(step);
  };

  // replay automatically after every run
  useEffect(() => {
    const id = requestAnimationFrame(play);
    return () => {
      cancelAnimationFrame(id);
      if (raf.current) cancelAnimationFrame(raf.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runKey]);

  const now = t0 + pos * span;
  // rows produced so far per operator, and which operators emitted recently
  const { rowsNow, active } = useMemo(() => {
    const rowsNow = new Map<number, number>();
    const active = new Set<number>();
    const window = span * 0.04;
    for (const [t, op, rows] of trace) {
      if (t > now) break;
      rowsNow.set(op, rows);
      if (pos < 1 && t > now - window) active.add(op);
    }
    return { rowsNow, active };
  }, [trace, now, pos, span]);

  const final = pos >= 1;
  const slow = Math.max(1, Math.round(REPLAY_MS / span));

  return (
    <div>
      <div className="plan-wrap">
        <svg
          className="plan-svg"
          viewBox={`${-PAD} ${-PAD} ${width} ${height}`}
          width={width}
          style={{ maxWidth: "none", display: "block" }}
          role="img"
          aria-label="Physical query plan with rows produced and time per operator"
        >
          {placed.map((p) =>
            p.parent ? (
              <path
                key={`e${p.n.id}`}
                d={
                  p.x === p.parent.x
                    ? `M${p.x + W / 2},${p.y} V${p.parent.y + H}`
                    : `M${p.x + W / 2},${p.y} V${p.y - GY / 2} H${p.parent.x + W - 18} V${p.parent.y + H}`
                }
                fill="none"
                stroke={active.has(p.n.id) ? "var(--accent)" : "var(--ink-2)"}
                strokeWidth={active.has(p.n.id) ? 2 : 1}
              />
            ) : null,
          )}
          {placed.map((p) => {
            const on = active.has(p.n.id);
            const rows = final ? p.n.rows : rowsNow.get(p.n.id) ?? 0;
            const started = final || rowsNow.has(p.n.id);
            return (
              <g key={p.n.id} transform={`translate(${p.x},${p.y})`}>
                <title>{`${p.n.op} ${p.n.detail}\n${fmtInt(p.n.rows)} rows in ${p.n.batches} batches\ntime in operator ${fmtMs(p.n.self_ms)}, including inputs ${fmtMs(p.n.ms)}`}</title>
                <rect
                  width={W}
                  height={H}
                  fill={on ? "var(--accent-tint)" : "#ffffff"}
                  stroke={on ? "var(--accent)" : "var(--ink)"}
                  strokeWidth={on ? 2 : 1}
                />
                <text x={8} y={16} fontSize={11.5} fontWeight={600} fill={on ? "var(--accent)" : "var(--ink)"}>
                  {p.n.op}
                </text>
                <text x={8} y={32} fontSize={10} fill="var(--ink-3)">
                  {clip(p.n.detail, 27)}
                </text>
                <line x1={0} x2={W} y1={40} y2={40} stroke="var(--rule)" />
                <text x={8} y={54} fontSize={10.5} fill={started ? "var(--ink)" : "var(--ink-3)"}>
                  {fmtInt(rows)} rows
                </text>
                <text x={W - 8} y={54} fontSize={10.5} textAnchor="end" fill="var(--ink-2)">
                  {final ? fmtMs(p.n.self_ms) : ""}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
      <div className="replay">
        <button className="btn ghost" onClick={play} disabled={playing || !trace.length}>
          {playing ? "Replaying" : "Replay"}
        </button>
        <input
          type="range"
          min={0}
          max={1000}
          value={Math.round(pos * 1000)}
          onChange={(e) => {
            if (raf.current) cancelAnimationFrame(raf.current);
            setPlaying(false);
            setPos(Number(e.target.value) / 1000);
          }}
          aria-label="Position in the execution trace"
        />
        <span className="tiny">
          {fmtMs(Math.max(0, now - t0))} of {fmtMs(span)}
          {trace.length ? `, slowed ${fmtInt(slow)}×` : ""}
        </span>
      </div>
    </div>
  );
}
