// The engine lives in a Web Worker (engine.worker.ts). This module wraps it in
// promises and can stop a running query by replacing the worker.
import type { QueryResult } from "./types";

export type Progress = { step: string; done: number; total: number };
export type TableInfo = { name: string; rows: number; columns: [string, string][] };

export interface Engine {
  run(sql: string, optimize: boolean, maxRows?: number): Promise<QueryResult>;
  script(sql: string): Promise<void>;
  /** Kill the worker (and whatever it is running) and boot a fresh one. */
  restart(onProgress?: (p: Progress) => void): Promise<void>;
  close(): void;
  tables: TableInfo[];
  wasmBytes: number;
  loadMs: number;
}

type Pending = { resolve: (json: string) => void; reject: (e: Error) => void };

export function createEngine(withData: boolean, onProgress: (p: Progress) => void = () => {}): Promise<Engine> {
  let worker: Worker;
  let nextId = 1;
  const pending = new Map<number, Pending>();

  const api: Engine = {
    tables: [],
    wasmBytes: 0,
    loadMs: 0,
    run(sql, optimize, maxRows = 500) {
      return call({ type: "query", sql, optimize, maxRows }).then((j) => JSON.parse(j) as QueryResult);
    },
    script(sql) {
      return call({ type: "script", sql }).then(() => undefined);
    },
    async restart(p = onProgress) {
      worker.terminate();
      for (const x of pending.values()) x.reject(new Error("stopped"));
      pending.clear();
      await start(p);
    },
    close() {
      worker.terminate();
    },
  };

  function call(msg: Record<string, unknown>): Promise<string> {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      worker.postMessage({ ...msg, id });
    });
  }

  function start(p: (x: Progress) => void): Promise<void> {
    return new Promise((resolve, reject) => {
      worker = new Worker(new URL("./engine.worker.ts", import.meta.url), { type: "module" });
      worker.onerror = (e) => reject(new Error(e.message || "the engine worker failed to start"));
      worker.onmessage = (ev) => {
        const m = ev.data;
        if (m.type === "progress") p({ step: m.step, done: m.done, total: m.total });
        else if (m.type === "ready") {
          api.tables = m.tables;
          api.wasmBytes = m.wasmBytes;
          api.loadMs = m.loadMs;
          resolve();
        } else if (m.type === "result" || m.type === "error") {
          const w = pending.get(m.id);
          if (m.type === "error" && m.id === -1) reject(new Error(m.message));
          if (!w) return;
          pending.delete(m.id);
          if (m.type === "result") w.resolve(m.json);
          else w.reject(new Error(m.message));
        }
      };
      worker.postMessage({ type: "init", withData });
    });
  }

  return start(onProgress).then(() => api);
}

let shared: Promise<Engine> | null = null;

/** The workbench engine, with the TPC-H SF 0.01 sample loaded. */
export function loadEngine(onProgress: (p: Progress) => void): Promise<Engine> {
  if (!shared) shared = createEngine(true, onProgress);
  return shared;
}
