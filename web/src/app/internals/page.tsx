import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Internals",
  description:
    "How Quackless works: a hand-written parser, a binder that decorrelates subqueries, a rule-based optimizer with cost-based join ordering, and pull-based vectorized operators over 2,048-row column batches.",
  alternates: { canonical: "/internals/" },
};

const STAGES = [
  { name: "Lexer", file: "lexer.rs", lines: 141, out: "tokens" },
  { name: "Parser", file: "parser.rs", lines: 918, out: "AST" },
  { name: "Binder / planner", file: "planner.rs", lines: 978, out: "logical plan" },
  { name: "Optimizer", file: "optimizer.rs", lines: 672, out: "logical plan" },
  { name: "Physical planner", file: "exec.rs", lines: 1240, out: "operators" },
  { name: "Execution", file: "eval.rs", lines: 906, out: "batches" },
];

function Architecture() {
  const bw = 150;
  const bh = 64;
  const gap = 34;
  const W = STAGES.length * bw + (STAGES.length - 1) * gap + 2;
  return (
    <svg viewBox={`-1 -24 ${W} 182`} width="100%" style={{ minWidth: 760, display: "block" }} role="img" aria-label="Pipeline from SQL text to result batches">
      <defs>
        <marker id="arr" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
          <path d="M0,0 L8,4 L0,8 Z" fill="#111214" />
        </marker>
      </defs>
      <text x={0} y={-10} fontSize={11} fontFamily="var(--mono)" fill="#5b6067">
        SQL text
      </text>
      {STAGES.map((s, i) => {
        const x = i * (bw + gap);
        const last = i === STAGES.length - 1;
        return (
          <g key={s.name}>
            <rect x={x} y={0} width={bw} height={bh} fill={last ? "#eaf0fd" : "#fff"} stroke={last ? "#1747d1" : "#111214"} strokeWidth={last ? 1.6 : 1} />
            <text x={x + 10} y={22} fontSize={14} fontWeight={600} fontFamily="var(--serif)" fill="#111214">
              {s.name}
            </text>
            <text x={x + 10} y={42} fontSize={10.5} fontFamily="var(--mono)" fill="#393d43">
              {s.file}
            </text>
            <text x={x + 10} y={56} fontSize={10.5} fontFamily="var(--mono)" fill="#5b6067">
              {s.lines.toLocaleString("en-US")} lines
            </text>
            {!last && (
              <>
                <line x1={x + bw} x2={x + bw + gap - 2} y1={bh / 2} y2={bh / 2} stroke="#111214" markerEnd="url(#arr)" />
              </>
            )}
            <text x={x + bw / 2} y={bh + 20} fontSize={10.5} textAnchor="middle" fontFamily="var(--mono)" fill="#5b6067">
              → {s.out}
            </text>
          </g>
        );
      })}
      <g transform={`translate(0, ${bh + 44})`}>
        <rect x={0} y={0} width={W - 2} height={46} fill="#f4f4f0" stroke="#dcdcd6" />
        <text x={10} y={19} fontSize={12} fontFamily="var(--serif)" fontWeight={600} fill="#111214">
          Catalog (catalog.rs)
        </text>
        <text x={10} y={36} fontSize={10.5} fontFamily="var(--mono)" fill="#393d43">
          tables as typed column vectors · views · exact distinct counts per column, computed on first use for join ordering
        </text>
      </g>
    </svg>
  );
}

export default function Internals() {
  return (
    <main className="wrap paper">
      <header className="page-head">
        <p className="kicker">Internals</p>
        <h1>How a query runs</h1>
        <div className="abstract">
          <p>
            <b>Overview.</b> Quackless is about 7,400 lines of Rust in its core crate, with no SQL library: the
            lexer, parser, binder, optimizer and operators are my own. The only dependency is the <code>csv</code>{" "}
            crate for reading input. The same crate compiles to a native CLI and, through <code>wasm-bindgen</code>,
            to the 727 KB WebAssembly module behind the <Link href="/">workbench</Link>.
          </p>
        </div>
      </header>

      <figure style={{ marginTop: 26 }}>
        <div className="chart-wrap">
          <Architecture />
        </div>
        <figcaption>
          The pipeline. Each stage is a plain function from one representation to the next; the workbench shows
          the logical plan before and after the optimizer and the operators as they ran. Line counts are for the
          files in <code>crates/core/src</code>.
        </figcaption>
      </figure>

      <h2>Columns and batches</h2>
      <p>
        A table is a list of typed column vectors: <code>Vec&lt;i64&gt;</code>, <code>Vec&lt;f64&gt;</code>,{" "}
        <code>Vec&lt;i32&gt;</code> for dates (days since 1970), booleans, and strings stored as one byte buffer
        plus offsets, each with an optional validity mask for NULLs. Operators pass batches of up to 2,048 rows.
        I wrote my own vectors instead of using arrow-rs: the engine needs only these physical types, and leaving
        Arrow out keeps the WebAssembly download small.
      </p>
      <p>
        <code>DECIMAL</code> is stored as a 64-bit float. That is the main compromise in the type system, and it is
        why the conformance checks compare non-integer numbers with a relative tolerance of 10<sup>−9</sup>.
      </p>

      <h2>Parsing and binding</h2>
      <p>
        The parser is hand-written recursive descent with one function per precedence level (OR, AND, NOT,
        comparison and postfix predicates, concatenation, addition, multiplication, unary). It covers SELECT with
        joins, derived tables and CTEs, GROUP BY / HAVING, ORDER BY / LIMIT / OFFSET, set operations, CASE, CAST,
        EXTRACT, INTERVAL arithmetic, IN, BETWEEN, LIKE, EXISTS, scalar subqueries, plus CREATE TABLE / VIEW and
        INSERT for the test suites.
      </p>
      <p>
        The binder resolves names, pulls aggregates out of expressions into an aggregate node, and decorrelates
        subqueries: <code>EXISTS</code> and <code>IN</code> become semi joins, <code>NOT EXISTS</code> an anti join,
        and a correlated scalar subquery (TPC-H Q2, Q17, Q20) becomes an aggregate joined back on the correlated
        columns. Subqueries it cannot rewrite are run per distinct value of their outer columns, with the results
        cached.
      </p>

      <h2>The optimizer</h2>
      <p>Rule-based, applied in a fixed order. Each rule records what it changed, which is the list under the plan in the workbench.</p>
      <ul>
        <li>
          <b>Constant folding</b> evaluates any expression whose inputs are all literals, with the same arithmetic
          rules as execution (a mismatch here was a real bug; see the conformance page), and simplifies{" "}
          <code>TRUE AND x</code>, <code>FALSE OR x</code> and arithmetic on NULL.
        </li>
        <li>
          <b>Predicate pushdown</b> splits WHERE into conjuncts and moves each as close to the scans as it can go,
          including into derived tables and the preserved side of outer joins. It also factors common terms out of
          ORs (<code>(a AND x) OR (a AND y)</code> becomes <code>a AND (x OR y)</code>), which is what lets Q19
          use a hash join.
        </li>
        <li>
          <b>Join ordering</b> flattens a region of inner joins, starts from the largest input, and then greedily
          adds whichever connected input gives the smallest estimated result, |L| × |R| / max(distinct keys), using
          exact distinct counts of the key columns. Equality predicates become hash join keys; anything else stays
          as a residual filter on the join.
        </li>
        <li>
          <b>Projection pruning</b> walks down from the output and removes every column nobody reads, down to the
          list of columns each scan touches.
        </li>
      </ul>

      <h2>Operators</h2>
      <p>
        Execution is pull-based: each operator&apos;s <code>next()</code> returns the next batch or nothing. Every
        operator is wrapped in an instrumented shell that counts rows and batches and times each call, which is
        where the per-operator numbers and the replay trace in the workbench come from.
      </p>
      <p className="tab-cap">The physical operators.</p>
      <div className="scroll-x">
        <table className="bt">
          <thead>
            <tr>
              <th>Operator</th>
              <th>How it works</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td className="m">SEQ_SCAN</td>
              <td>Slices the stored columns into 2,048-row batches; only the columns projection pruning kept.</td>
            </tr>
            <tr>
              <td className="m">FILTER</td>
              <td>Evaluates the predicate over the batch into a boolean column, then gathers the selected rows.</td>
            </tr>
            <tr>
              <td className="m">PROJECTION</td>
              <td>Evaluates each expression over the whole batch at once.</td>
            </tr>
            <tr>
              <td className="m">HASH_JOIN</td>
              <td>
                Drains the right input into a hash table on the key columns, then streams the left input through it.
                Handles inner, left, semi and anti joins and a residual filter.
              </td>
            </tr>
            <tr>
              <td className="m">HASH_GROUP_BY</td>
              <td>Encodes group keys to bytes, maps them to group ids, and updates SUM, COUNT, AVG, MIN, MAX and COUNT(DISTINCT) accumulators per batch.</td>
            </tr>
            <tr>
              <td className="m">ORDER_BY, LIMIT</td>
              <td>Sorts an index over the materialized input with a multi-key comparator; LIMIT stops pulling early.</td>
            </tr>
            <tr>
              <td className="m">NESTED_LOOP_JOIN</td>
              <td>For joins with no equality key; also the cross product.</td>
            </tr>
            <tr>
              <td className="m">UNION, EXCEPT, INTERSECT</td>
              <td>Hash sets over encoded rows.</td>
            </tr>
          </tbody>
        </table>
      </div>
      <p>
        Expressions are compiled once per query into a tree that works on whole columns. CASE and COALESCE evaluate
        each branch only on the rows that reach it: a condition splits the batch into a selection of row ids, the
        branch runs on that selection, and the pieces are scattered back into one column. The fuzzer found that the
        first version evaluated every branch on every row.
      </p>

      <h2>Known limits</h2>
      <ul>
        <li>Single-threaded, everything in memory, no indexes and no persistent storage.</li>
        <li>DECIMAL is a 64-bit float, not an exact decimal.</li>
        <li>Correlated subqueries can reference the query directly around them, not one two levels up.</li>
        <li>Subqueries inside aggregate arguments (<code>sum((SELECT …))</code>) are not supported.</li>
        <li>
          Whether <code>x % 0</code> raises an error depends on which rows get evaluated. Quackless and DuckDB
          evaluate filters in different orders, so they can disagree on whether a query errors.
        </li>
        <li>No window functions, no UPDATE or DELETE, no timestamps or time zones.</li>
      </ul>
    </main>
  );
}
