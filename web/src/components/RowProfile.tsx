import type { PhysNode } from "@/lib/types";
import { fmtInt, fmtMs } from "@/lib/format";

type Flat = { n: PhysNode; depth: number };

function flatten(n: PhysNode, depth = 0, out: Flat[] = []) {
  out.push({ n, depth });
  n.children.forEach((c) => flatten(c, depth + 1, out));
  return out;
}

/** Rows each operator produced, in plan order, as a table with bars. */
export default function RowProfile({ root, returned }: { root: PhysNode; returned: number }) {
  const ops = flatten(root);
  const max = Math.max(1, ...ops.map((o) => o.n.rows));
  const scanned = ops.filter((o) => o.n.op === "SEQ_SCAN").reduce((a, o) => a + o.n.rows, 0);
  const moved = ops.reduce((a, o) => a + o.n.rows, 0);
  return (
    <div className="profile">
      <p className="small" style={{ marginTop: 0 }}>
        The scans read <b>{fmtInt(scanned)}</b> rows, the operators passed <b>{fmtInt(moved)}</b> rows between them
        in total, and <b>{fmtInt(returned)}</b> came out. Indentation follows the plan; the bar is rows produced.
      </p>
      <table className="bt prof">
        <thead>
          <tr>
            <th>Operator</th>
            <th aria-hidden="true" />
            <th className="n">Rows</th>
            <th className="n">Time</th>
          </tr>
        </thead>
        <tbody>
          {ops.map(({ n, depth }) => (
            <tr key={n.id} title={`${n.op} ${n.detail}`}>
              <td>
                <div className="prof-op" style={{ paddingLeft: Math.min(depth, 8) * 10 }}>
                  <span className="m">{n.op}</span>
                  <span className="prof-detail">{n.detail}</span>
                </div>
              </td>
              <td className="prof-bar" aria-hidden="true">
                <span style={{ width: `${Math.max(n.rows > 0 ? 1.5 : 0, (100 * n.rows) / max)}%` }} />
              </td>
              <td className="n">{fmtInt(n.rows)}</td>
              <td className="n">{fmtMs(n.self_ms)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="tiny">
        Time is spent inside the operator, inputs excluded. Browsers round timers to about 0.1 ms, so at this data
        size most operators read &ldquo;&lt; 0.1 ms&rdquo;; the row counts are exact.
      </p>
    </div>
  );
}
