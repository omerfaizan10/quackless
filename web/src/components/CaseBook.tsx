"use client";

import { useRef, useState } from "react";
import { createEngine, type Engine } from "@/lib/engine";
import { isOrdered, rowsEqual } from "@/lib/compare";
import { fmtInt } from "@/lib/format";

export type Case = {
  id: string;
  setup: string;
  query: string;
  expected: (string | number | boolean | null)[][];
  rows: Record<string, number>;
  tags: string[];
  kind?: string;
};

type Verdict = { state: "match" | "differ" | "error"; detail: string; ours?: unknown[][] };

let shared: Promise<Engine> | null = null;
const bare = () => (shared ??= createEngine(false));

async function check(c: Case): Promise<Verdict> {
  const e = await bare();
  try {
    await e.script(`DROP TABLE IF EXISTS t1; DROP TABLE IF EXISTS t2;\n${c.setup}`);
    const r = await e.run(c.query, true, 100000);
    const ok = rowsEqual(r.rows, c.expected, isOrdered(c.query));
    return {
      state: ok ? "match" : "differ",
      detail: ok ? `same ${fmtInt(r.row_count)} rows as DuckDB` : `${fmtInt(r.row_count)} rows, not DuckDB's answer`,
      ours: r.rows,
    };
  } catch (err) {
    return { state: "error", detail: err instanceof Error ? err.message : String(err) };
  }
}

function show(rows: unknown[][], n = 6) {
  const head = rows.slice(0, n).map((r) => JSON.stringify(r)).join("\n");
  return rows.length > n ? `${head}\n… ${rows.length - n} more` : head || "(no rows)";
}

export default function CaseBook({ cases, label }: { cases: Case[]; label: string }) {
  const tags = Array.from(new Set(cases.flatMap((c) => c.tags))).sort();
  const [tag, setTag] = useState<string | null>(null);
  const [verdicts, setVerdicts] = useState<Record<string, Verdict>>({});
  const [running, setRunning] = useState<string | null>(null);
  const [all, setAll] = useState<{ done: number; match: number } | null>(null);
  const stopAll = useRef(false);

  const shown = tag ? cases.filter((c) => c.tags.includes(tag)) : cases;

  const one = async (c: Case) => {
    setRunning(c.id);
    const v = await check(c);
    setVerdicts((m) => ({ ...m, [c.id]: v }));
    setRunning(null);
    return v;
  };

  const runAll = async () => {
    stopAll.current = false;
    let done = 0;
    let match = 0;
    setAll({ done, match });
    for (const c of cases) {
      if (stopAll.current) break;
      const v = await one(c);
      done++;
      if (v.state === "match") match++;
      setAll({ done, match });
    }
  };

  return (
    <div className="casebook">
      <div className="case-tools">
        <button className="btn" onClick={runAll} disabled={running !== null}>
          Re-check all {cases.length} here
        </button>
        {all && (
          <span className="timing" role="status">
            {all.done} of {cases.length} run · {all.match} match DuckDB
          </span>
        )}
      </div>
      <div className="chips" role="group" aria-label={`Filter ${label} by what the query contains`}>
        <button aria-pressed={tag === null} onClick={() => setTag(null)}>
          all {cases.length}
        </button>
        {tags.map((t) => (
          <button key={t} aria-pressed={tag === t} onClick={() => setTag(tag === t ? null : t)}>
            {t} {cases.filter((c) => c.tags.includes(t)).length}
          </button>
        ))}
      </div>
      <ol className="cases">
        {shown.map((c) => {
          const v = verdicts[c.id];
          return (
            <li key={c.id} className="case">
              <div className="case-head">
                <span className="m">{c.id}</span>
                <span className="tiny">
                  {Object.entries(c.rows)
                    .map(([t, n]) => `${t}: ${n} rows`)
                    .join(" · ")}
                  {c.tags.length > 0 && <> · {c.tags.join(", ")}</>}
                </span>
              </div>
              <pre className="sql">{c.query}</pre>
              <div className="case-foot">
                <details>
                  <summary className="tiny">Setup and DuckDB&apos;s answer</summary>
                  <pre className="out">{c.setup}</pre>
                  <pre className="out">{show(c.expected)}</pre>
                </details>
                <div className="case-run">
                  {v && (
                    <span className={`verdict ${v.state}`}>
                      {v.state === "match" ? "matches" : v.state === "differ" ? "differs" : "error"}: {v.detail}
                    </span>
                  )}
                  <button className="btn ghost sm" onClick={() => one(c)} disabled={running !== null}>
                    {running === c.id ? "Running" : v ? "Run again" : "Re-check here"}
                  </button>
                </div>
              </div>
              {v?.state === "differ" && v.ours && <pre className="out">Quackless:{"\n" + show(v.ours)}</pre>}
            </li>
          );
        })}
      </ol>
    </div>
  );
}
