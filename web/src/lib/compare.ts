// The comparison rules of crates/harness/src/compare.rs, for re-checking saved
// fuzzer cases in the browser. JSON loses the int/float distinction, so every
// number is compared with the float rule.
import type { Cell } from "./types";

type Expected = Cell | string;

function special(v: unknown): string | null {
  if (typeof v === "string") {
    const s = v.toLowerCase();
    if (s === "inf" || s === "-inf" || s === "nan") return s;
  }
  if (typeof v === "number" && !Number.isFinite(v)) return Number.isNaN(v) ? "nan" : v > 0 ? "inf" : "-inf";
  return null;
}

export function cellsEqual(a: unknown, b: unknown): boolean {
  if (a === null || b === null) return a === b;
  const sa = special(a);
  const sb = special(b);
  if (sa || sb) return sa === sb;
  if (typeof a === "number" && typeof b === "number") {
    if (a === b) return true;
    const scale = Math.max(Math.abs(a), Math.abs(b));
    return scale < 1 ? Math.abs(a - b) <= 1e-9 : Math.abs(a - b) / scale <= 1e-9;
  }
  return String(a) === String(b);
}

/** True when the outer query (outside any parentheses) has ORDER BY. */
export function isOrdered(sql: string) {
  let depth = 0;
  let outer = "";
  for (const ch of sql) {
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    else if (depth === 0) outer += ch;
  }
  return /\border\s+by\b/i.test(outer);
}

function key(row: unknown[]) {
  return JSON.stringify(
    row.map((v) => {
      const s = special(v);
      if (s) return s;
      return typeof v === "number" ? Number(v.toPrecision(9)) : v;
    }),
  );
}

export function rowsEqual(ours: unknown[][], expected: Expected[][], ordered: boolean) {
  if (ours.length !== expected.length) return false;
  const a = ordered ? ours : [...ours].sort((x, y) => (key(x) < key(y) ? -1 : key(x) > key(y) ? 1 : 0));
  const b = ordered ? expected : [...expected].sort((x, y) => (key(x) < key(y) ? -1 : key(x) > key(y) ? 1 : 0));
  return a.every((r, i) => r.length === b[i].length && r.every((v, j) => cellsEqual(v, b[i][j])));
}
