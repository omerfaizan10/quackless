// Dot plot on a log scale: one row per TPC-H query, one mark per engine.
// Palette validated with the dataviz checker (lightness, chroma, CVD, contrast);
// every engine also has its own marker shape, so color is never the only cue.

type Row = { q: number; quackless_ms: number; duckdb_1t_ms: number; duckdb_mt_ms: number; sqlite_ms: number | null };

export const SERIES = [
  { key: "quackless_ms", label: "Quackless (1 thread)", color: "#1747d1", shape: "circle" },
  { key: "duckdb_1t_ms", label: "DuckDB, 1 thread", color: "#c2571a", shape: "square" },
  { key: "duckdb_mt_ms", label: "DuckDB, all threads", color: "#00876f", shape: "diamond" },
  { key: "sqlite_ms", label: "SQLite (no indexes)", color: "#9a7400", shape: "triangle" },
] as const;

function Marker({ shape, x, y, color }: { shape: string; x: number; y: number; color: string }) {
  const r = 4.5;
  if (shape === "square") return <rect x={x - r} y={y - r} width={2 * r} height={2 * r} fill={color} stroke="#fff" strokeWidth={1.5} />;
  if (shape === "diamond")
    return <path d={`M${x},${y - r - 1} L${x + r + 1},${y} L${x},${y + r + 1} L${x - r - 1},${y} Z`} fill={color} stroke="#fff" strokeWidth={1.5} />;
  if (shape === "triangle")
    return <path d={`M${x},${y - r - 1} L${x + r + 1},${y + r} L${x - r - 1},${y + r} Z`} fill={color} stroke="#fff" strokeWidth={1.5} />;
  return <circle cx={x} cy={y} r={r + 0.5} fill={color} stroke="#fff" strokeWidth={1.5} />;
}

export function LegendKey({ shape, color }: { shape: string; color: string }) {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <Marker shape={shape} x={7} y={7} color={color} />
    </svg>
  );
}

export default function BenchChart({ rows, capS }: { rows: Row[]; capS: number }) {
  const L = 52;
  const R = 74;
  const W = 760;
  const rowH = 22;
  const top = 26;
  const H = top + rows.length * rowH + 8;
  const lo = 0.5;
  const hi = capS * 1000;
  const x = (ms: number) => L + ((Math.log10(ms) - Math.log10(lo)) / (Math.log10(hi) - Math.log10(lo))) * (W - L - R);
  const ticks = [1, 10, 100, 1000, 10000];
  return (
    <svg viewBox={`0 0 ${W} ${H}`} width="100%" style={{ minWidth: 560, display: "block" }} role="img" aria-label="Median time per TPC-H query for each engine, log scale">
      {ticks.map((t) => (
        <g key={t}>
          <line x1={x(t)} x2={x(t)} y1={top - 6} y2={H - 6} stroke="#e7e7e2" />
          <text x={x(t)} y={top - 12} fontSize={11} textAnchor="middle" fill="#5b6067" fontFamily="var(--mono)">
            {t >= 1000 ? `${t / 1000} s` : `${t} ms`}
          </text>
        </g>
      ))}
      <text x={x(hi)} y={top - 12} fontSize={11} textAnchor="middle" fill="#5b6067" fontFamily="var(--mono)">
        {capS} s cap
      </text>
      <line x1={x(hi)} x2={x(hi)} y1={top - 6} y2={H - 6} stroke="#c4c4bd" strokeDasharray="3 3" />
      {rows.map((r, i) => {
        const y = top + i * rowH + rowH / 2;
        const vals = SERIES.map((s) => r[s.key] as number | null).filter((v): v is number => v !== null);
        return (
          <g key={r.q}>
            <text x={L - 10} y={y + 4} fontSize={11.5} textAnchor="end" fill="#111214" fontFamily="var(--mono)">
              Q{r.q}
            </text>
            <line x1={x(Math.min(...vals))} x2={x(Math.max(...vals))} y1={y} y2={y} stroke="#c4c4bd" />
            {SERIES.map((s) => {
              const v = r[s.key] as number | null;
              if (v === null)
                return (
                  <g key={s.key}>
                    <title>{`Q${r.q}, ${s.label}: did not finish within ${capS} s`}</title>
                    <Marker shape={s.shape} x={x(hi)} y={y} color={s.color} />
                    <text x={x(hi) + 10} y={y + 4} fontSize={10.5} fill="#393d43" fontFamily="var(--mono)">
                      &gt; {capS} s
                    </text>
                  </g>
                );
              return (
                <g key={s.key}>
                  <title>{`Q${r.q}, ${s.label}: ${v < 10 ? v.toFixed(2) : v.toFixed(0)} ms`}</title>
                  <circle cx={x(Math.max(v, lo))} cy={y} r={10} fill="transparent" />
                  <Marker shape={s.shape} x={x(Math.max(v, lo))} y={y} color={s.color} />
                </g>
              );
            })}
          </g>
        );
      })}
    </svg>
  );
}
