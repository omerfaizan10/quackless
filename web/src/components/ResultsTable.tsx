import type { QueryResult } from "@/lib/types";
import { fmtCell, fmtInt, isNumericType } from "@/lib/format";

export default function ResultsTable({ r, limit = 200 }: { r: QueryResult; limit?: number }) {
  if (r.affected !== null) return <p className="small">OK, {fmtInt(r.affected)} rows affected.</p>;
  const shown = r.rows.slice(0, limit);
  return (
    <div className="scroll-x">
      <table className="bt">
        <thead>
          <tr>
            {r.columns.map((c, i) => (
              <th key={i} className={isNumericType(c.type) ? "n" : undefined} title={c.type}>
                {c.name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {shown.map((row, i) => (
            <tr key={i}>
              {row.map((v, j) => {
                const num = isNumericType(r.columns[j].type);
                return (
                  <td key={j} className={v === null ? "null" : num ? "n" : "m"}>
                    {fmtCell(v, r.columns[j].type)}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      {r.row_count > shown.length && (
        <p className="tiny">
          Showing {fmtInt(shown.length)} of {fmtInt(r.row_count)} rows.
        </p>
      )}
    </div>
  );
}
