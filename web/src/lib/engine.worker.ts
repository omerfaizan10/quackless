// Runs the WebAssembly engine off the main thread, so a slow query never
// freezes the page. Messages in: init, query, script. Messages out: progress,
// ready, result, error.
import init, { Engine } from "@/wasm-pkg/quackless_wasm.js";

type In =
  | { type: "init"; withData: boolean }
  | { type: "query"; id: number; sql: string; optimize: boolean; maxRows: number }
  | { type: "script"; id: number; sql: string };

let engine: Engine | null = null;

const post = (m: unknown) => (self as unknown as Worker).postMessage(m);

async function boot(withData: boolean) {
  const t0 = performance.now();
  post({ type: "progress", step: "engine (quackless_wasm_bg.wasm)", done: 0, total: 10 });
  const buf = await (await fetch("/quackless_wasm_bg.wasm")).arrayBuffer();
  await init({ module_or_path: buf });
  engine = new Engine();
  if (withData) {
    post({ type: "progress", step: "schema.sql", done: 1, total: 10 });
    engine.execute_script(await (await fetch("/data/schema.sql")).text());
    const names: string[] = JSON.parse(engine.tables()).map((t: { name: string }) => t.name);
    let done = 2;
    await Promise.all(
      names.map(async (name) => {
        const bytes = new Uint8Array(await (await fetch(`/data/${name}.csv`)).arrayBuffer());
        engine!.load_csv(name, bytes);
        done += 1;
        post({ type: "progress", step: `${name}.csv`, done, total: names.length + 2 });
      }),
    );
  }
  post({ type: "ready", tables: JSON.parse(engine.tables()), wasmBytes: buf.byteLength, loadMs: performance.now() - t0 });
}

self.onmessage = async (ev: MessageEvent<In>) => {
  const m = ev.data;
  try {
    if (m.type === "init") return await boot(m.withData);
    if (!engine) throw new Error("engine not loaded");
    if (m.type === "query") {
      const json = engine.query(m.sql, m.optimize, m.maxRows);
      post({ type: "result", id: m.id, json });
    } else if (m.type === "script") {
      engine.execute_script(m.sql);
      post({ type: "result", id: m.id, json: "null" });
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    post({ type: "error", id: "id" in m ? m.id : -1, message: msg });
  }
};
