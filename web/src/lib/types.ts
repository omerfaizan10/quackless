export type Cell = string | number | boolean | null;

export interface LogicalNode {
  op: string;
  detail: string;
  children: LogicalNode[];
}

export interface PhysNode {
  id: number;
  op: string;
  detail: string;
  rows: number;
  batches: number;
  /** inclusive time (ms), children included */
  ms: number;
  /** time spent in this operator alone (ms) */
  self_ms: number;
  children: PhysNode[];
}

export interface QueryResult {
  columns: { name: string; type: string }[];
  row_count: number;
  rows: Cell[][];
  plan_before: LogicalNode | null;
  plan_after: LogicalNode | null;
  physical: PhysNode | null;
  /** [t_ms since start, operator id, cumulative rows for that operator] */
  trace: [number, number, number][];
  optimizer: { rule: string; change: string }[];
  timing: { parse_ms: number; plan_ms: number; optimize_ms: number; exec_ms: number };
  affected: number | null;
}

export interface StoredQuery {
  id: string;
  group: string;
  title: string;
  sql: string;
}
