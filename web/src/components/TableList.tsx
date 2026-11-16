import type { TableInfo } from "@/lib/engine";
import { fmtInt } from "@/lib/format";

/** The tables loaded in the browser, each with a button that queries it. */
export default function TableList({ tables, onQuery }: { tables: TableInfo[]; onQuery: (sql: string) => void }) {
  const sorted = [...tables].sort((a, b) => b.rows - a.rows);
  return (
    <div className="tables">
      {sorted.map((t) => (
        <details key={t.name}>
          <summary>
            <span className="m">{t.name}</span>
            <span className="tiny">
              {fmtInt(t.rows)} rows · {t.columns.length} columns
            </span>
          </summary>
          <div className="cols">
            {t.columns.map(([c, ty]) => (
              <span key={c}>
                {c} <i>{ty}</i>
              </span>
            ))}
          </div>
          <button className="btn ghost sm" onClick={() => onQuery(`SELECT *\nFROM ${t.name}\nLIMIT 20`)}>
            Query {t.name}
          </button>
        </details>
      ))}
    </div>
  );
}
