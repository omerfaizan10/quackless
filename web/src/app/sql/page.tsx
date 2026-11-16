import type { Metadata } from "next";
import Link from "next/link";
import reference from "@/data/reference.json";

export const metadata: Metadata = {
  title: "SQL reference",
  description:
    "The SQL dialect Quackless understands: joins, grouping, subqueries, set operations, dates, strings and numbers, each with an example and the engine's own output, plus what it does not support.",
  alternates: { canonical: "/sql/" },
};

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");

export default function SqlReference() {
  const { sections, unsupported } = reference;
  const count = sections.reduce((a, s) => a + s.examples.length, 0);
  return (
    <main className="wrap paper">
      <header className="page-head">
        <p className="kicker">SQL reference</p>
        <h1>The dialect, by example</h1>
        <div className="abstract">
          <p>
            <b>Scope.</b> Quackless reads a subset of SQL close to DuckDB&apos;s: enough for all 22 TPC-H queries and
            most of SQLite&apos;s sqllogictest suite. Each of the {count} examples below was run through the engine
            over the TPC-H sample when this site was built (<code>scripts/export_pages.py</code>), and the output
            shown is the engine&apos;s own. Every example opens in the <Link href="/">workbench</Link>, where you can
            change it and see its plan.
          </p>
        </div>
      </header>

      <nav className="toc" aria-label="Sections">
        <ol>
          {sections.map((s) => (
            <li key={s.title}>
              <a href={`#${slug(s.title)}`}>{s.title}</a>
            </li>
          ))}
          <li>
            <a href="#not-supported">Not supported</a>
          </li>
        </ol>
      </nav>

      {sections.map((s) => (
        <section key={s.title}>
          <h2 id={slug(s.title)}>{s.title}</h2>
          <p>{s.intro}</p>
          {s.examples.map((e) => (
            <div className="example" key={e.label}>
              <div className="example-head">
                <h3>{e.label}</h3>
                <Link href={`/?sql=${encodeURIComponent(e.sql)}`} className="tiny-link">
                  Open in workbench
                </Link>
              </div>
              <div className="example-body">
                <pre className="sql">{e.sql}</pre>
                <pre className="out" aria-label="Output">
                  {e.output}
                </pre>
              </div>
            </div>
          ))}
        </section>
      ))}

      <h2 id="not-supported">Not supported</h2>
      <p>
        These are outside the dialect. The messages are what the engine says when you try, also captured at build
        time.
      </p>
      <div className="scroll-x">
        <table className="bt">
          <thead>
            <tr>
              <th>Feature</th>
              <th>Statement</th>
              <th>Engine says</th>
            </tr>
          </thead>
          <tbody>
            {unsupported.map((u) => (
              <tr key={u.label}>
                <td>{u.label}</td>
                <td className="m small-code">{u.sql}</td>
                <td className="m small-code">{u.error}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="small">
        Also missing: indexes, persistent storage, transactions and more than one thread. <code>DECIMAL</code> is
        stored as a 64-bit float. Correlated subqueries can reference the query directly around them, not one two
        levels up. The <Link href="/internals/">internals</Link> page has the full list of known limits.
      </p>
    </main>
  );
}
