"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import Editor from "./Editor";
import PlanTree from "./PlanTree";
import ResultsTable from "./ResultsTable";
import LogicalDiff from "./LogicalDiff";
import RowProfile from "./RowProfile";
import TableList from "./TableList";
import { loadEngine, type Engine, type Progress } from "@/lib/engine";
import type { QueryResult, StoredQuery } from "@/lib/types";
import { fmtInt, fmtMs } from "@/lib/format";
import queries from "@/data/queries.json";

const QUERIES = queries as StoredQuery[];
const DEFAULT = "q5";
const CUSTOM = "custom";

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
  // whether the result on screen was produced with the optimizer on
  const [ranOpt, setRanOpt] = useState(true);
  const [running, setRunning] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const seq = useRef(0);
  const busy = useRef(false);

  const run = useCallback(
    async (e: Engine | null, text: string, opt: boolean) => {
      if (!e) return;
      const my = ++seq.current;
      // the worker runs one query at a time: replace it if one is still going
      if (busy.current) {
        setRestarting(true);
        await e.restart(setProgress);
        setRestarting(false);
        if (my !== seq.current) return;
      }
      busy.current = true;
      setRunning(true);
      const t = performance.now();
      try {
        const r = await e.run(text, opt);
        if (my !== seq.current) return;
        setWall(performance.now() - t);
        setResult(r);
        setRanOpt(opt);
        setError(null);
        setRunKey((k) => k + 1);
      } catch (err) {
        if (my !== seq.current) return;
        const msg = err instanceof Error ? err.message : String(err);
        if (msg !== "stopped") {
          setError(msg);
          setResult(null);
        }
      } finally {
        if (my === seq.current) {
          busy.current = false;
          setRunning(false);
        }
      }
    },
    [],
  );

  const stop = async () => {
    if (!engine) return;
    seq.current++;
    busy.current = false;
    setRunning(false);
    setRestarting(true);
    await engine.restart(setProgress);
    setRestarting(false);
    setError("Stopped. The engine was restarted with the sample data reloaded.");
  };

  useEffect(() => {
    // ?q=q5 opens a stored query, ?sql=... opens any text
    const params = new URLSearchParams(window.location.search);
    const qp = params.get("q");
    const sp = params.get("sql");
    const stored = qp ? QUERIES.find((q) => q.id === qp) : undefined;
    loadEngine(setProgress)
      .then((e) => {
        let text = QUERIES.find((q) => q.id === DEFAULT)!.sql;
        if (stored) {
          text = stored.sql;
          setPicked(stored.id);
          setSql(text);
        } else if (sp) {
          text = sp;
          setPicked(CUSTOM);
          setSql(text);
        }
        setEngine(e);
        run(e, text, true);
      })
      .catch((err) => setBootError(err instanceof Error ? err.message : String(err)));
  }, [run]);

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

  const openSql = (text: string) => {
    setPicked(CUSTOM);
    setSql(text);
    run(engine, text, optimize);
  };

  const title = QUERIES.find((q) => q.id === picked && q.sql === sql)?.title ?? "your query";
  const groups = Array.from(new Set(QUERIES.map((q) => q.group)));

  return (
    <div className="workbench">
      <section aria-labelledby="q-title">
        <div className="panel-title">
          <h2 id="q-title">Query</h2>
          <select className="picker" value={picked} onChange={(e) => pick(e.target.value)} aria-label="Example queries">
            {picked === CUSTOM && <option value={CUSTOM}>Your query</option>}
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
        <Editor value={sql} onChange={setSql} onRun={() => run(engine, sql, optimize)} />
        <div className="controls">
          <button className="btn" onClick={() => run(engine, sql, optimize)} disabled={!engine || restarting}>
            Run <span aria-hidden="true">⌘↵</span>
          </button>
          <span className="seg" role="group" aria-label="Optimizer">
            <button aria-pressed={optimize} onClick={() => toggleOpt(true)} disabled={!engine}>
              Optimizer on
            </button>
            <button aria-pressed={!optimize} onClick={() => toggleOpt(false)} disabled={!engine}>
              off
            </button>
          </span>
          {running && !restarting && (
            <span className="timing" role="status">
              Running…{" "}
              <button className="linkish" onClick={stop}>
                Stop
              </button>
            </span>
          )}
          {restarting && (
            <span className="timing" role="status">
              Restarting the engine ({progress.done} of {progress.total})
            </span>
          )}
          {!running && !restarting && result && !error && (
            <span className="timing">
              {fmtInt(result.row_count)} rows · execute {fmtMs(result.timing.exec_ms)} · total {fmtMs(wall)}
            </span>
          )}
        </div>
        {!optimize && (
          <p className="note">
            Without the optimizer every join is a nested loop over all pairs of rows, and filters run after the
            joins. Queries that list several tables with commas (Q5, Q7, Q8 and most of TPC-H) are stopped after
            10 million row pairs instead of running for minutes.
          </p>
        )}
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
            <ResultsTable r={result} limit={50} />
          </div>
        )}
        {result?.physical && (
          <div className="results">
            <div className="panel-title">
              <h2>Where the rows went</h2>
              <span className="tiny">{ranOpt ? "optimizer on" : "optimizer off"}</span>
            </div>
            <RowProfile root={result.physical} returned={result.row_count} />
          </div>
        )}
        {engine && (
          <div className="results">
            <div className="panel-title">
              <h2>Tables in your browser</h2>
              <span className="tiny">TPC-H, scale factor 0.01</span>
            </div>
            <TableList tables={engine.tables} onQuery={openSql} />
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
              <b>Figure 1.</b> Physical plan for {title}, {ranOpt ? "after" : "without"} the optimizer. Rows flow
              upward from the scans in batches of up to 2,048. Each box shows the rows that operator produced and
              the time spent inside it, not counting its inputs. The replay is the engine&apos;s own execution
              trace, slowed down; the operator producing rows is drawn in blue.
            </figcaption>
          </figure>
        )}
        {result && view === "logical" && (
          <figure>
            {ranOpt ? (
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
        {result && ranOpt && result.optimizer.length > 0 && (
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
