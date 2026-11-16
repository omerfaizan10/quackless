import type { Metadata } from "next";
import Link from "next/link";
import CaseBook, { type Case } from "@/components/CaseBook";
import casebook from "@/data/casebook.json";

export const metadata: Metadata = {
  title: "Fuzzer casebook",
  description:
    "Every disagreement the differential fuzzer found between Quackless and DuckDB, minimized and saved as a test, and re-checkable in your browser.",
  alternates: { canonical: "/fuzzer/" },
};

const fmt = (n: number) => n.toLocaleString("en-US");

export default function Fuzzer() {
  const fixed = casebook.fixed as Case[];
  const open = casebook.open as Case[];
  const seeds = casebook.seeds;
  const total = seeds.reduce((a, s) => a + s.queries, 0);
  const agree = seeds.reduce((a, s) => a + s.agree, 0);
  const secs = seeds.reduce((a, s) => a + s.seconds, 0);
  const fixedDiv = fixed.filter((c) => c.tags.includes("division") || c.tags.includes("modulo")).length;
  return (
    <main className="wrap paper">
      <header className="page-head">
        <p className="kicker">Fuzzer casebook</p>
        <h1>Every bug the fuzzer found</h1>
        <div className="abstract">
          <p>
            <b>Summary.</b> A generator writes random queries over two small tables; each runs in Quackless and in
            DuckDB, and any disagreement is shrunk to the smallest query that still disagrees and saved. Earlier runs
            produced the {fixed.length} cases in section 2, all fixed and now part of <code>cargo test</code>. The final
            run of {fmt(total)} queries found no new disagreements. The buttons below run each saved case again in
            the engine compiled for your browser and compare the answer with DuckDB&apos;s.
          </p>
        </div>
      </header>

      <h2 id="method">How it works</h2>
      <ol>
        <li>
          <b>Tables.</b> Every 250 queries the generator makes two fresh tables, <code>t1(a, b, c, d)</code> and <code>t2(a, e, f)</code>, with
          small integers, a few fruit names, doubles and NULLs in every column, so joins match some rows and miss
          others.
        </li>
        <li>
          <b>Queries.</b> A typed expression tree is grown at random: arithmetic, comparisons, CASE, COALESCE,
          NULLIF, LIKE, IN lists, IN and EXISTS subqueries, aggregates with GROUP BY and HAVING, joins of every
          kind, DISTINCT, ORDER BY with LIMIT, and UNION, EXCEPT and INTERSECT. The grammar stays inside what both
          engines accept; in the final run DuckDB rejected one query of 50,000.
        </li>
        <li>
          <b>Compare.</b> Both engines run the query; the answers are compared with the same rules as the{" "}
          <Link href="/conformance/#rules">conformance checks</Link>.
        </li>
        <li>
          <b>Shrink.</b> On a disagreement, the shrinker tries dropping a clause (LIMIT, ORDER BY, DISTINCT, HAVING,
          WHERE, a set operation, an output column) and replacing an expression with one of its parts, keeping each
          change that still disagrees, until no single change does or 400 attempts are used up.
        </li>
        <li>
          <b>Save.</b> The minimized case is written to <code>tests/fuzz/open/</code> with DuckDB&apos;s answer.
          When a fix makes it pass, <code>make fuzz</code> moves it to <code>tests/fuzz/</code>, where{" "}
          <code>cargo test</code> runs it from then on.
        </li>
      </ol>

      <p className="tab-cap">The final run, one row per seed.</p>
      <div className="scroll-x">
        <table className="bt" style={{ maxWidth: 680 }}>
          <thead>
            <tr>
              <th className="n">Seed</th>
              <th className="n">Queries</th>
              <th className="n">Same answer</th>
              <th className="n">DuckDB rejected</th>
              <th className="n">Disagreements</th>
              <th className="n">Seconds</th>
            </tr>
          </thead>
          <tbody>
            {seeds.map((s) => (
              <tr key={s.seed}>
                <td className="n">{s.seed}</td>
                <td className="n">{fmt(s.queries)}</td>
                <td className="n">{fmt(s.agree)}</td>
                <td className="n">{s.rejected_by_duckdb}</td>
                <td className="n">{s.mismatches}</td>
                <td className="n">{s.seconds.toFixed(1)}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td>All</td>
              <td className="n">{fmt(total)}</td>
              <td className="n">{fmt(agree)}</td>
              <td className="n">{seeds.reduce((a, s) => a + s.rejected_by_duckdb, 0)}</td>
              <td className="n">{seeds.reduce((a, s) => a + s.mismatches, 0)}</td>
              <td className="n">{secs.toFixed(0)}</td>
            </tr>
          </tfoot>
        </table>
      </div>

      <h2 id="fixed">The {fixed.length} fixed cases</h2>
      <p>
        Each case is the query as the shrinker left it. The tags say what the query contains, not what the bug
        was. {fixedDiv === fixed.length ? "Every one" : `${fixedDiv} of them`} contains a division or a modulo: the
        early grammar let divisors be zero, and dividing by zero is where Quackless first differed most from DuckDB.
        The fixes behind them:
      </p>
      <ul>
        <li>CASE and COALESCE now evaluate each branch only on the rows that reach it.</li>
        <li>NaN sorts above every number in ORDER BY, MIN and MAX, as in DuckDB.</li>
        <li>
          <code>1 / 0</code> is infinity and <code>x % 0</code> is an error, as in DuckDB; arithmetic on a NULL
          literal folds to NULL before anything is evaluated.
        </li>
        <li>
          <code>IS [NOT] DISTINCT FROM</code> is parsed.
        </li>
      </ul>
      <CaseBook cases={fixed} label="fixed cases" />

      <h2 id="open">The {open.length} open cases</h2>
      <p>These still disagree with DuckDB, for two reasons I chose not to chase:</p>
      <ul>
        <li>
          <b>x % 0 ({open.filter((c) => c.kind === "x % 0").length} cases).</b> Whether the query raises an error
          depends on which rows reach the modulo. DuckDB filters and joins in a different order, so one engine can
          error on a row the other never evaluates. Neither answer is wrong.
        </li>
        <li>
          <b>Correlation two levels deep ({open.filter((c) => c.kind === "two-level correlation").length} cases).</b>{" "}
          A subquery inside a subquery that refers to the outermost query. The binder only decorrelates one level,
          so Quackless rejects these.
        </li>
      </ul>
      <p className="small">
        The final grammar wraps divisors in <code>nullif(…, 0)</code> and nests subqueries one level, so these do not
        come up any more.
      </p>
      <CaseBook cases={open} label="open cases" />
    </main>
  );
}
