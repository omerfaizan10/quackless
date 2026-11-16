import Link from "next/link";
import Workbench from "@/components/Workbench";
import conformance from "@/data/conformance.json";

export default function Home() {
  const sf = conformance.tpch?.scale_factors ?? [];
  const tpchAll = sf.length > 0 && sf.every((s) => s.passed === s.total);
  const slt = conformance.slt;
  const fuzz = conformance.fuzz;
  return (
    <main className="wrap">
      <div className="bench-head">
        <div>
          <h1>A SQL engine I wrote in Rust, running in your browser</h1>
          <p className="lede">
            Quackless parses, plans, optimizes and executes SQL over columns in batches. It is compiled to
            WebAssembly and has the TPC-H sample (scale factor 0.01, {sumRows(conformance.sample_rows)} rows) loaded
            below. Pick a query or write one.
          </p>
        </div>
        <div className="facts" aria-label="Conformance summary">
          {tpchAll && (
            <Link href="/conformance/#tpch">
              <b>22/22</b> TPC-H match DuckDB at SF {sf.map((s) => s.sf).join(", ")}
            </Link>
          )}
          {slt && (
            <Link href="/conformance/#slt">
              <b>{((100 * slt.passed) / slt.records).toFixed(2)}%</b> of {fmt(slt.records)} sqllogictest records
            </Link>
          )}
          {fuzz && (
            <Link href="/conformance/#fuzz">
              <b>{fmt(fuzz.queries)}</b> fuzzed queries
            </Link>
          )}
        </div>
      </div>
      <Workbench />
    </main>
  );
}

function fmt(n: number) {
  return n.toLocaleString("en-US");
}

function sumRows(t: Record<string, number>) {
  return fmt(Object.values(t).reduce((a, b) => a + b, 0));
}
