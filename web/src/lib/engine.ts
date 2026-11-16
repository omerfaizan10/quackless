// Loads the WebAssembly engine and the TPC-H SF 0.01 sample into it.
import type { QueryResult } from "./types";

type WasmEngine = {
  execute_script(sql: string): void;
  load_csv(table: string, bytes: Uint8Array): number;
  query(sql: string, optimize: boolean, maxRows: number): string;
  tables(): string;
};

export type Progress = { step: string; done: number; total: number };

export interface Engine {
  run(sql: string, optimize: boolean): QueryResult;
  tables: { name: string; rows: number; columns: [string, string][] }[];
  wasmBytes: number;
  loadMs: number;
}

let loading: Promise<Engine> | null = null;

export function loadEngine(onProgress: (p: Progress) => void): Promise<Engine> {
  if (!loading) loading = boot(onProgress);
  return loading;
}

async function boot(onProgress: (p: Progress) => void): Promise<Engine> {
  const t0 = performance.now();
  onProgress({ step: "engine (quackless_wasm_bg.wasm)", done: 0, total: 10 });
  const mod = await import("@/wasm-pkg/quackless_wasm.js");
  const wasmResp = await fetch("/quackless_wasm_bg.wasm");
  const wasmBuf = await wasmResp.arrayBuffer();
  await mod.default({ module_or_path: wasmBuf });
  const e = new mod.Engine() as unknown as WasmEngine;
  onProgress({ step: "schema.sql", done: 1, total: 10 });
  e.execute_script(await (await fetch("/data/schema.sql")).text());
  const names: string[] = JSON.parse(e.tables()).map((t: { name: string }) => t.name);
  let done = 2;
  // fetch in parallel, load in order of arrival
  await Promise.all(
    names.map(async (name) => {
      const buf = new Uint8Array(await (await fetch(`/data/${name}.csv`)).arrayBuffer());
      e.load_csv(name, buf);
      done += 1;
      onProgress({ step: `${name}.csv`, done, total: names.length + 2 });
    }),
  );
  return {
    run(sql: string, optimize: boolean) {
      return JSON.parse(e.query(sql, optimize, 500)) as QueryResult;
    },
    tables: JSON.parse(e.tables()),
    wasmBytes: wasmBuf.byteLength,
    loadMs: performance.now() - t0,
  };
}
