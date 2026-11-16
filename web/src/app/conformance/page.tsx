import type { Metadata } from "next";
import Link from "next/link";
import BenchChart, { LegendKey, SERIES } from "@/components/BenchChart";
import conformance from "@/data/conformance.json";
import queries from "@/data/queries.json";

export const metadata: Metadata = {
  title: "Conformance",
  description:
    "How Quackless is checked against DuckDB: all 22 TPC-H queries at three scale factors, SQLite's sqllogictest suite, a differential fuzzer and benchmarks against DuckDB and SQLite.",
  alternates: { canonical: "/conformance/" },
};

const fmt = (n: number) => n.toLocaleString("en-US");
const pct = (a: number, b: number) => ((100 * a) / b).toFixed(2) + "%";
const ms = (v: number | null) => (v === null ? "" : v < 10 ? v.toFixed(2) : v < 100 ? v.toFixed(1) : v.toFixed(0));

function geomean(xs: number[]) {
  return Math.exp(xs.reduce((a, x) => a + Math.log(x), 0) / xs.length);
}

export default function Conformance() {
  const { tpch, slt, fuzz, bench } = conformance;
  const titles = Object.fromEntries(queries.filter((q) => q.group === "TPC-H").map((q) => [Number(q.id.slice(1)), q.title.replace(/^Q\d+\. /, "")]));
  const sfs = tpch?.scale_factors ?? [];
  const sf1 = sfs.find((s) => s.sf === "1");

  const rows = bench?.queries ?? [];
  const vsDuck1 = geomean(rows.map((r) => r.quackless_ms / r.duckdb_1t_ms));
  const vsDuckN = geomean(rows.map((r) => r.quackless_ms / r.duckdb_mt_ms));
  const liteRows = rows.filter((r) => r.sqlite_ms !== null);
  const vsLite = geomean(liteRows.map((r) => (r.sqlite_ms as number) / r.quackless_ms));
  const liteDnf = rows.filter((r) => r.sqlite_ms === null).map((r) => `Q${r.q}`);
  const sf1Ours = sf1 ? sf1.queries.reduce((a, q) => a + q.quackless_ms, 0) : 0;
  const sf1Duck = sf1 ? sf1.queries.reduce((a, q) => a + q.duckdb_ms, 0) : 0;

  return (
    <main className="wrap paper">
      <header className="page-head">
        <p className="kicker">Conformance report</p>
        <h1>Is it right, and how fast is it?</h1>
        <div className="abstract">
          <p>
            <b>Summary.</b> Every answer below is computed by a second engine and compared row by row. Quackless
            returns the same result as DuckDB {duckdbVersion(bench)} for all 22 TPC-H queries at scale factors{" "}
            {sfs.map((s) => s.sf).join(", ")}
            {slt && (
              <>
                , passes {fmt(slt.passed)} of {fmt(slt.records)} sqllogictest records ({pct(slt.passed, slt.records)})
              </>
            )}
            {fuzz && (
              <>
                , and agreed with DuckDB on {fmt(fuzz.agree)} of {fmt(fuzz.queries)} random queries in the final fuzzing run
              </>
            )}
            . It is single-threaded and {vsDuck1.toFixed(1)} times slower than DuckDB running on one thread (geometric
            mean over the 22 queries at SF {bench?.sf}).
          </p>
        </div>
      </header>

      <h2 id="tpch">TPC-H, differential against DuckDB</h2>
      <p>
        The data comes from DuckDB&apos;s own <code>tpch</code> extension (<code>CALL dbgen(sf=…)</code>), exported to CSV.
        Both engines load the same files and run the 22 queries exactly as the extension ships them. A query passes
        when every row matches, under the rules in section 5.
      </p>
      <p className="tab-cap">
        TPC-H results. Times at SF 1 ({sf1 ? fmt(6001215) : ""} lineitem rows) are single runs on an Apple Silicon
        laptop; DuckDB uses all cores here. Section 4 has the controlled benchmark.
      </p>
      <div className="scroll-x">
        <table className="bt">
          <thead>
            <tr>
              <th>Query</th>
              <th>What it asks</th>
              {sfs.map((s) => (
                <th key={s.sf} className="n">
                  SF {s.sf}
                </th>
              ))}
              <th className="n">Rows (SF 1)</th>
              <th className="n">Quackless, ms</th>
              <th className="n">DuckDB, ms</th>
            </tr>
          </thead>
          <tbody>
            {Array.from({ length: 22 }, (_, i) => i + 1).map((q) => {
              const big = sf1?.queries.find((x) => x.q === q);
              return (
                <tr key={q}>
                  <td className="m">Q{q}</td>
                  <td>{titles[q]}</td>
                  {sfs.map((s) => {
                    const r = s.queries.find((x) => x.q === q);
                    return (
                      <td key={s.sf} className={r?.status === "pass" ? "n" : "n fail"}>
                        {r?.status === "pass" ? "match" : r?.status ?? ""}
                      </td>
                    );
                  })}
                  <td className="n">{big ? fmt(big.rows) : ""}</td>
                  <td className="n">{big ? ms(big.quackless_ms) : ""}</td>
                  <td className="n">{big ? ms(big.duckdb_ms) : ""}</td>
                </tr>
              );
            })}
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={2}>Matches</td>
              {sfs.map((s) => (
                <td key={s.sf} className="n">
                  {s.passed} / {s.total}
                </td>
              ))}
              <td colSpan={3} />
            </tr>
          </tfoot>
        </table>
      </div>
      <p className="small">
        Running SF 1 is what found the worst bug in the optimizer: the join orderer picked the next table by input
        size alone, so Q5 joined lineitem to customer on nation key, a many-to-many join of roughly 1.2 million by
        6,000 rows, and never finished. It now estimates each join&apos;s output from exact distinct counts of the key
        columns and Q5 runs in about 150 ms.
      </p>

      {slt && (
        <>
          <h2 id="slt">sqllogictest</h2>
          <p>
            sqllogictest is the suite SQLite uses to compare SQL engines: scripts of statements and queries with the
            expected output. I run a fixed subset of {slt.files} files (list in <code>scripts/fetch_slt.py</code>, pinned
            to one commit) in SQLite mode, where integer division truncates. The denominator is every record the
            files mark as runnable on SQLite (records under <code>skipif sqlite</code> or{" "}
            <code>onlyif</code> another engine are left out); a record passes when its output, formatted the way
            SQLite&apos;s harness formats it, matches exactly or by MD5 hash.
          </p>
          <div className="two">
            <div>
              <p className="tab-cap">sqllogictest pass rate by part of the suite.</p>
              <table className="bt">
                <thead>
                  <tr>
                    <th>Part</th>
                    <th className="n">Files</th>
                    <th className="n">Records</th>
                    <th className="n">Pass</th>
                  </tr>
                </thead>
                <tbody>
                  {slt.groups.map((g) => (
                    <tr key={g.group}>
                      <td className="m">{g.group}</td>
                      <td className="n">{g.files}</td>
                      <td className="n">{fmt(g.records)}</td>
                      <td className="n">{pct(g.passed, g.records)}</td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr>
                    <td>All</td>
                    <td className="n">{slt.files}</td>
                    <td className="n">{fmt(slt.records)}</td>
                    <td className="n">{pct(slt.passed, slt.records)}</td>
                  </tr>
                </tfoot>
              </table>
            </div>
            <div>
              <p className="tab-cap">Why the {fmt(slt.records - slt.passed)} failing records fail.</p>
              <table className="bt">
                <thead>
                  <tr>
                    <th>Reason</th>
                    <th className="n">Records</th>
                  </tr>
                </thead>
                <tbody>
                  {slt.failure_categories.map((c) => (
                    <tr key={c.category}>
                      <td>{c.category}</td>
                      <td className="n">{fmt(c.records)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
          <p className="small">
            Most of what is left is SQLite being more permissive than DuckDB or PostgreSQL: it lets a grouped query
            select a column that is not grouped, accepts <code>x IN tablename</code> and an empty <code>IN ()</code>,
            and types values one at a time, so <code>COALESCE(-81, AVG(x))</code> is an integer when the first argument
            wins. Quackless types expressions statically, like DuckDB. Running this suite also found a real bug: the
            optimizer folded constants with DuckDB&apos;s division rules even in SQLite mode, so <code>57 / 36</code>{" "}
            became 1.58 at plan time but 1 at run time. Fixing it moved the pass rate from 99.10% to{" "}
            {pct(slt.passed, slt.records)}.
          </p>
        </>
      )}

      {fuzz && (
        <>
          <h2 id="fuzz">Differential fuzzing</h2>
          <p>
            A small generator (<code>crates/harness/src/fuzz.rs</code>) writes random, well-typed queries inside the
            dialect: filters, joins (inner, left, cross), derived tables, grouping with HAVING, DISTINCT, ORDER BY
            with LIMIT, UNION / EXCEPT / INTERSECT, CASE, COALESCE, NULLIF, LIKE, IN lists, IN and EXISTS subqueries,
            and arithmetic, over two small tables with NULLs in every column. Each query runs in both engines. When
            they disagree, the query is shrunk greedily (drop a clause, replace an expression by a child or a literal)
            until nothing smaller still disagrees, and saved with DuckDB&apos;s answer as a test.
          </p>
          <p className="tab-cap">The final fuzzing run (seeds {fuzz.seeds.slice().sort((a, b) => a - b).join(", ")}, 5,000 queries each).</p>
          <table className="bt" style={{ maxWidth: 560 }}>
            <tbody>
              <tr>
                <td>Queries generated</td>
                <td className="n">{fmt(fuzz.queries)}</td>
              </tr>
              <tr>
                <td>Same answer as DuckDB</td>
                <td className="n">{fmt(fuzz.agree)}</td>
              </tr>
              <tr>
                <td>Both engines raised an error</td>
                <td className="n">{fmt(fuzz.both_error)}</td>
              </tr>
              <tr>
                <td>Rejected by DuckDB only (outside the shared dialect)</td>
                <td className="n">{fmt(fuzz.rejected_by_duckdb)}</td>
              </tr>
              <tr>
                <td>Disagreements</td>
                <td className="n">{fmt(fuzz.mismatches)}</td>
              </tr>
            </tbody>
          </table>
          <h3>What earlier runs found</h3>
          <p>
            The first runs of the wider grammar disagreed with DuckDB on about 90 of every 5,000 queries. Each
            disagreement became a minimized test; {fuzz.fixed_regressions} of them now pass and run with{" "}
            <code>cargo test</code>. The fixes:
          </p>
          <ul>
            <li>
              <b>CASE and COALESCE evaluated every branch on every row.</b> <code>CASE WHEN b = 0 THEN 0 ELSE a % b END</code>{" "}
              raised a division-by-zero error from a branch no row takes. Both now evaluate each branch only on the
              rows that reach it, using selection vectors, which is how a vectorized engine has to do it.
            </li>
            <li>
              <b>NaN ordering.</b> DuckDB sorts NaN above every number, so <code>max(0/0)</code> is NaN; Quackless
              skipped it. One float ordering is now shared by ORDER BY, MIN and MAX.
            </li>
            <li>
              <b>Division semantics.</b> DuckDB returns <code>inf</code> for <code>1 / 0</code> and raises an error for{" "}
              <code>7 % 0</code>; Quackless returned NULL for both. It now follows DuckDB, and keeps NULL in SQLite mode.
            </li>
            <li>
              <b>NULL in arithmetic.</b> <code>NULL - (b % 0)</code> is NULL in DuckDB because it never evaluates the
              right side. Quackless now folds arithmetic on a NULL literal the same way.
            </li>
            <li>
              <b>Missing syntax.</b> <code>IS [NOT] DISTINCT FROM</code> was not parsed; it is now rewritten to a
              NULL-safe CASE.
            </li>
          </ul>
          <p>
            {fuzz.open} saved disagreements are still open, in two groups I decided not to chase:{" "}
            {fuzz.open_kinds.map((k, i) => (
              <span key={k.kind}>
                {i > 0 ? "; " : ""}
                {k.kind} ({k.count})
              </span>
            ))}
            . The final grammar keeps divisors nonzero with <code>nullif</code> and nests subqueries one level, and
            the README lists both as known limits.
          </p>
        </>
      )}

      {bench && (
        <>
          <h2 id="perf">Performance</h2>
          <p>
            Quackless is slower than DuckDB, and it should be: DuckDB has years of work in compressed storage,
            morsel-driven parallelism and specialized kernels. The comparison is here to show where the gap is, on
            the same machine, data and queries. Each query runs {bench.runs} times after the data is loaded; the
            median wall time is shown. SQLite gets the same CSVs with dates as ISO text and no indexes, and its answers
            are checked against DuckDB&apos;s too.
          </p>
          <figure>
            <div className="chart-wrap">
              <div className="legend">
                {SERIES.map((s) => (
                  <span key={s.key}>
                    <LegendKey shape={s.shape} color={s.color} />
                    {s.label}
                  </span>
                ))}
              </div>
              <div className="scroll-x">
                <BenchChart rows={rows} capS={bench.sqlite_cap_s ?? 60} />
              </div>
            </div>
            <figcaption>
              Median time per TPC-H query at SF {bench.sf} (log scale; hover a mark for its value). On the geometric
              mean Quackless is {vsDuck1.toFixed(1)}× slower than one-thread DuckDB and {vsDuckN.toFixed(1)}× slower than
              DuckDB on all {bench.cpu_count} threads, and {vsLite.toFixed(0)}× faster than SQLite without indexes
              {liteDnf.length > 0 && <> ({liteDnf.join(", ")} excluded: SQLite did not finish within {bench.sqlite_cap_s} s)</>}.
            </figcaption>
          </figure>
          {sf1 && (
            <p className="small">
              SF {bench.sf} is small enough that fixed costs blur the gap. On the six-million-row SF 1 run in section 1,
              the 22 queries take {(sf1Ours / 1000).toFixed(1)} s in total in Quackless and {(sf1Duck / 1000).toFixed(2)} s
              in DuckDB on all cores, about {Math.round(sf1Ours / sf1Duck)}× apart.
            </p>
          )}
          <details style={{ marginTop: 14 }}>
            <summary className="small">The numbers as a table</summary>
            <div className="scroll-x">
              <table className="bt" style={{ marginTop: 8 }}>
                <thead>
                  <tr>
                    <th>Query</th>
                    <th className="n">Quackless</th>
                    <th className="n">DuckDB 1 thread</th>
                    <th className="n">DuckDB {bench.cpu_count} threads</th>
                    <th className="n">SQLite</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => (
                    <tr key={r.q}>
                      <td className="m">Q{r.q}</td>
                      <td className="n">{ms(r.quackless_ms)}</td>
                      <td className="n">{ms(r.duckdb_1t_ms)}</td>
                      <td className="n">{ms(r.duckdb_mt_ms)}</td>
                      <td className="n">{r.sqlite_ms === null ? `> ${bench.sqlite_cap_s} s` : ms(r.sqlite_ms)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="tiny">Milliseconds. DuckDB {bench.duckdb_version}, SQLite {bench.sqlite_version}.</p>
            </div>
          </details>
          <p className="small">
            The same 22 queries are also a criterion benchmark (<code>cargo bench -p quackless-core</code>) for
            tracking the engine against itself.
          </p>
        </>
      )}

      <h2 id="rules">How answers are compared</h2>
      <ul>
        <li>NULL equals only NULL.</li>
        <li>Integers on both sides must be equal. Strings and dates (as ISO text) must be equal byte for byte.</li>
        <li>
          Any other number is compared with a relative tolerance of 10<sup>−9</sup> (absolute 10<sup>−9</sup> near
          zero). Quackless stores <code>DECIMAL</code> as a 64-bit float while DuckDB keeps an exact decimal, so a sum
          over six million rows can differ in the last bits. Infinities must match exactly.
        </li>
        <li>
          Without an <code>ORDER BY</code> on the outer query, rows are compared as a multiset. With one, order
          matters, and a result with the right rows in the wrong order fails.
        </li>
      </ul>
      <p className="small">
        The comparison code is <code>crates/harness/src/compare.rs</code>, with its own tests. Every number on this
        page is written by <code>make eval</code> into <code>results/</code>; see the{" "}
        <Link href="/internals/">internals</Link> for how the engine works.
      </p>
    </main>
  );
}

function duckdbVersion(bench: { duckdb_version?: string } | null) {
  return bench?.duckdb_version ?? "";
}
