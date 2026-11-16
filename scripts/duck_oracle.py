"""DuckDB oracle for differential testing.

Reads one JSON request per line on stdin, writes one JSON reply per line.

  {"op": "load_tpch", "dir": "data/sf0.01"}   create the TPC-H tables from
                                              data/schema.sql and load the CSVs
  {"op": "query", "sql": "..."}               run SQL and return the rows
  {"op": "reset"}                             drop everything (fresh database)

Values are normalized so the Rust side can compare them: DECIMAL becomes a
float, DATE an ISO string, integers stay integers (as text beyond 2^53).
"""
import datetime
import decimal
import json
import math
import pathlib
import sys

import duckdb

ROOT = pathlib.Path(__file__).resolve().parent.parent


def norm(v):
    if v is None or isinstance(v, (bool, str)):
        return v
    if isinstance(v, int):
        return v if abs(v) < 2**53 else str(v)
    if isinstance(v, decimal.Decimal):
        return float(v)
    if isinstance(v, float):
        return v if math.isfinite(v) else str(v)
    if isinstance(v, (datetime.date, datetime.datetime)):
        return v.isoformat()
    return str(v)


def main():
    con = duckdb.connect()
    out = sys.stdout
    for line in sys.stdin:
        req = json.loads(line)
        try:
            op = req["op"]
            if op == "reset":
                con.close()
                con = duckdb.connect()
                reply = {"ok": True}
            elif op == "load_tpch":
                d = (ROOT / req["dir"]).resolve()
                con.execute((ROOT / "data" / "schema.sql").read_text())
                for (t,) in con.execute("SELECT table_name FROM duckdb_tables()").fetchall():
                    con.execute(f"COPY {t} FROM '{d / (t + '.csv')}' (HEADER)")
                reply = {"ok": True}
            else:
                rel = con.execute(req["sql"])
                if rel.description is None:
                    reply = {"ok": True, "columns": [], "types": [], "rows": []}
                else:
                    rows = rel.fetchall()
                    reply = {
                        "ok": True,
                        "columns": [c[0] for c in rel.description],
                        "types": [str(c[1]) for c in rel.description],
                        "rows": [[norm(v) for v in r] for r in rows],
                    }
        except Exception as e:  # report, keep serving
            reply = {"ok": False, "error": f"{type(e).__name__}: {e}"}
        out.write(json.dumps(reply) + "\n")
        out.flush()


if __name__ == "__main__":
    main()
