import type { Cell } from "./types";

export function fmtCell(v: Cell, type: string): string {
  if (v === null) return "NULL";
  if (typeof v === "number") {
    if (type === "DOUBLE") {
      if (!Number.isFinite(v)) return String(v);
      // DECIMAL columns are stored as DOUBLE; show at most 4 decimals
      return String(Math.round(v * 1e4) / 1e4);
    }
    return String(v);
  }
  return String(v);
}

export function fmtMs(ms: number): string {
  if (ms >= 100) return `${ms.toFixed(0)} ms`;
  if (ms >= 10) return `${ms.toFixed(1)} ms`;
  if (ms >= 0.1) return `${ms.toFixed(2)} ms`;
  // browser timers resolve to about 0.1 ms, so smaller values are not meaningful
  return "< 0.1 ms";
}

export function fmtInt(n: number): string {
  return n.toLocaleString("en-US");
}

export function isNumericType(t: string) {
  return t === "BIGINT" || t === "DOUBLE";
}
