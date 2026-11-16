import type { LogicalNode } from "@/lib/types";

function lines(n: LogicalNode | null, depth = 0, out: string[] = []): string[] {
  if (!n) return out;
  out.push(`${"  ".repeat(depth)}${n.op}${n.detail ? " " + n.detail : ""}`);
  n.children.forEach((c) => lines(c, depth + 1, out));
  return out;
}

/** The logical plan as the planner built it next to the optimizer's output; lines
 *  that appear on only one side are marked. */
export default function LogicalDiff({ before, after }: { before: LogicalNode | null; after: LogicalNode | null }) {
  const b = lines(before);
  const a = lines(after);
  const bs = new Set(b.map((l) => l.trim()));
  const as = new Set(a.map((l) => l.trim()));
  return (
    <div className="diff">
      <div>
        <h3>Before: planner output</h3>
        <pre>
          {b.map((l, i) => (
            <div key={i} className={as.has(l.trim()) ? undefined : "changed"}>
              {l}
            </div>
          ))}
        </pre>
      </div>
      <div>
        <h3>After: optimizer output</h3>
        <pre>
          {a.map((l, i) => (
            <div key={i} className={bs.has(l.trim()) ? undefined : "changed"}>
              {l}
            </div>
          ))}
        </pre>
      </div>
    </div>
  );
}
