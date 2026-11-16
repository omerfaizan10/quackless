"use client";

import { useCallback, useEffect, useState } from "react";
import Editor from "./Editor";
import PlanTree from "./PlanTree";
import ResultsTable from "./ResultsTable";
import LogicalDiff from "./LogicalDiff";
import { loadEngine, type Engine, type Progress } from "@/lib/engine";
import type { QueryResult, StoredQuery } from "@/lib/types";
import { fmtInt, fmtMs } from "@/lib/format";
import queries from "@/data/queries.json";

const QUERIES = queries as StoredQuery[];
const DEFAULT = "q5";

export default function Workbench() {
  const [engine, setEngine] = useState<Engine | null>(null);
  const [progress, setProgress] = useState<Progress>({ step: "starting", done: 0, total: 10 });
  const [bootError, setBootError] = useState<string | null>(null);
  const [picked, setPicked] = useState(DEFAULT);
  const [sql, setSql] = useState(QUERIES.find((q) => q.id === DEFAULT)!.sql);
  const [optimize, setOptimize] = useState(true);
  const [result, setResult] = useState<QueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<"physical" | "logical">("physical");
  const [runKey, setRunKey] = useState(0);
  const [wall, setWall] = useState(0);

  const run = useCallback(
    (e: Engine | null = engine, text = sql, opt = optimize) => {
      if (!e) return;
      const t = performance.now();
      try {
        const r = e.run(text, opt);
        setWall(performance.now() - t);
        setResult(r);
        setError(null);
        setRunKey((k) => k + 1);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [engine, sql, optimize],
  );

  useEffect(() => {
    loadEngine(setProgress)
      .then((e) => {
        setEngine(e);
        run(e);
      })
      .catch((err) => setBootError(String(err)));
    // load once
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pick = (id: string) => {
    const q = QUERIES.find((x) => x.id === id);
    if (!q) return;
    setPicked(id);
    setSql(q.sql);
    run(engine, q.sql, optimize);
  };

  const toggleOpt = (on: boolean) => {
    setOptimize(on);
    run(engine, sql, on);
  };

  const title = QUERIES.find((q) => q.id === picked && q.sql === sql)?.title ?? "your query";
  const groups = Array.from(new Set(QUERIES.map((q) => q.group)));

  return (
    <div className="workbench">
      <section aria-labelledby="q-title">
        <div className="panel-title">
          <h2 id="q-title">Query</h2>
          <select className="picker" value={picked} onChange={(e) => pick(e.target.value)} aria-label="Example queries">
            {groups.map((g) => (
              <optgroup key={g} label={g}>
                {QUERIES.filter((q) => q.group === g).map((q) => (
                  <option key={q.id} value={q.id}>
                    {q.title}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </div>
        <Editor value={sql} onChange={setSql} onRun={() => run()} />
        <div className="controls">
          <button className="btn" onClick={() => run()} disabled={!engine}>
            Run <span aria-hidden="true">⌘↵</span>
          </button>
          <span className="seg" role="group" aria-label="Optimizer">
            <button aria-pressed={optimize} onClick={() => toggleOpt(true)}>
              Optimizer on
            </button>
            <button aria-pressed={!optimize} onClick={() => toggleOpt(false)}>
              off
            </button>
          </span>
          {result && !error && (
            <span className="timing">
              {fmtInt(result.row_count)} rows · execute {fmtMs(result.timing.exec_ms)} · total {fmtMs(wall)}
            </span>
          )}
        </div>
        {bootError && <p className="err">The engine did not load: {bootError}</p>}
        {error && <p className="err">{error}</p>}
        {!engine && !bootError && (
          <p className="loading" role="status">
            Loading {progress.step} ({progress.done} of {progress.total})
          </p>
        )}
        {result && (
          <div className="results">
            <div className="panel-title">
              <h2>Result</h2>
              <span className="tiny">
                parse {fmtMs(result.timing.parse_ms)} · plan {fmtMs(result.timing.plan_ms)} · optimize{" "}
                {fmtMs(result.timing.optimize_ms)} · execute {fmtMs(result.timing.exec_ms)}
              </span>
            </div>
            <ResultsTable r={result} />
          </div>
        )}
      </section>

      <section aria-labelledby="plan-title">
        <div className="panel-title">
          <h2 id="plan-title">Plan</h2>
          <span className="seg" role="group" aria-label="Plan view">
            <button aria-pressed={view === "physical"} onClick={() => setView("physical")}>
              As executed
            </button>
            <button aria-pressed={view === "logical"} onClick={() => setView("logical")}>
              Optimizer before / after
            </button>
          </span>
        </div>
        {result?.physical && view === "physical" && (
          <figure>
            <PlanTree root={result.physical} trace={result.trace} runKey={runKey} />
            <figcaption>
              <b>Figure 1.</b> Physical plan for {title}, {optimize ? "after" : "without"} the optimizer. Rows flow
              upward from the scans in batches of up to 2,048. Each box shows the rows that operator produced and
              the time spent inside it, not counting its inputs. The replay is the engine&apos;s own execution
              trace, slowed down; the operator producing rows is drawn in blue.
            </figcaption>
          </figure>
        )}
        {result && view === "logical" && (
          <figure>
            {optimize ? (
              <LogicalDiff before={result.plan_before} after={result.plan_after} />
            ) : (
              <p className="small">The optimizer is off, so the plan runs exactly as the planner built it. Turn it on to compare.</p>
            )}
            <figcaption>
              <b>Figure 1.</b> Logical plan for {title}. Lines marked in blue exist on one side only: what
              predicate pushdown, join ordering, constant folding and projection pruning changed.
            </figcaption>
          </figure>
        )}
        {result && optimize && result.optimizer.length > 0 && (
          <>
            <div className="panel-title" style={{ marginTop: 22 }}>
              <h2>What the optimizer did</h2>
              <span className="tiny">{result.optimizer.length} rewrites</span>
            </div>
            <ul className="rules">
              {result.optimizer.slice(0, 40).map((o, i) => (
                <li key={i}>
                  <span>{o.rule}</span>
                  <span>{o.change}</span>
                </li>
              ))}
            </ul>
          </>
        )}
      </section>
    </div>
  );
}
