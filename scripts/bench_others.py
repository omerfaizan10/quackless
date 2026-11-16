"""Time the 22 TPC-H queries in DuckDB (1 thread and all threads) and SQLite.

Usage: .venv/bin/python -I scripts/bench_others.py [SF] [RUNS]   (default 0.1, 5)

Same method as `quackless-harness bench`: data loaded first (not timed), then
each query run RUNS times; the median wall time is reported. SQLite has no DATE
type, so dates are stored as ISO text and three constructs are rewritten
(date literals, extract(year ...), substring(... FROM ... FOR ...)), and q13's
derived-table column list becomes a column alias. SQLite's
answers are checked against DuckDB's so a broken rewrite cannot hide.
A SQLite query still running after 60 s is stopped and reported as not
finished (SQLite has no indexes here, so correlated subqueries rescan lineitem).
Writes results/bench_others_sf<SF>.json.
"""
import json
import math
import os
import pathlib
import re
import sqlite3
import statistics
import sys
import time

import duckdb

ROOT = pathlib.Path(__file__).resolve().parent.parent
SQLITE_CAP_S = 60
TABLES = ["region", "nation", "supplier", "customer", "part", "partsupp", "orders", "lineitem"]


def sqlite_sql(q):
    q = re.sub(r"CAST\('(\d{4}-\d{2}-\d{2})' AS date\)", r"'\1'", q)
    q = re.sub(r"\bdate '(\d{4}-\d{2}-\d{2})'", r"'\1'", q, flags=re.I)
    q = re.sub(r"extract\(year FROM (\w+)\)", r"CAST(strftime('%Y', \1) AS INTEGER)", q, flags=re.I)
    q = re.sub(r"substring\((\w+) FROM (\d+) FOR (\d+)\)", r"substr(\1, \2, \3)", q, flags=re.I)
    # q13: SQLite has no column list on a derived-table alias; name the column inside instead
    q = q.replace("count(o_orderkey)\n", "count(o_orderkey) AS c_count\n")
    q = re.sub(r"AS c_orders \(c_custkey,\s*c_count\)", "AS c_orders", q)
    return q


def timed(fn, runs):
    out, ts = None, []
    for _ in range(runs):
        t = time.perf_counter()
        out = fn()
        ts.append((time.perf_counter() - t) * 1000)
    return out, statistics.median(ts)


def same(a, b):
    if len(a) != len(b):
        return False
    key = lambda r: tuple("" if v is None else (round(float(v), 4) if isinstance(v, (int, float)) or hasattr(v, "as_tuple") else str(v)) for v in r)
    for x, y in zip(sorted(map(key, a)), sorted(map(key, b))):
        for u, v in zip(x, y):
            if isinstance(u, float) and isinstance(v, float):
                if not math.isclose(u, v, rel_tol=1e-6, abs_tol=1e-6):
                    return False
            elif str(u) != str(v):
                return False
    return True


def main():
    sf = sys.argv[1] if len(sys.argv) > 1 else "0.1"
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 5
    d = ROOT / "data" / f"sf{sf}"
    schema = (ROOT / "data" / "schema.sql").read_text()

    duck = duckdb.connect()
    duck.execute(schema)
    for t in TABLES:
        duck.execute(f"COPY {t} FROM '{d / (t + '.csv')}' (HEADER)")

    lite = sqlite3.connect(":memory:")
    lite_schema = re.sub(r"DECIMAL\(\d+,\d+\)", "REAL", schema).replace(" DATE ", " TEXT ")
    lite.executescript(lite_schema)
    for t in TABLES:
        rows = duck.execute(f"SELECT * FROM {t}").fetchall()
        rows = [tuple(v.isoformat() if hasattr(v, "isoformat") else (float(v) if hasattr(v, "as_tuple") else v) for v in r) for r in rows]
        lite.executemany(f"INSERT INTO {t} VALUES ({','.join('?' * len(rows[0]))})", rows)
    lite.commit()

    result = {"sqlite_cap_s": SQLITE_CAP_S, "sf": sf, "runs": runs, "cpu_count": os.cpu_count(), "duckdb_version": duckdb.__version__,
              "sqlite_version": sqlite3.sqlite_version, "queries": []}
    for q in range(1, 23):
        sql = (ROOT / "queries" / "tpch" / f"q{q:02d}.sql").read_text().strip().rstrip(";")
        duck.execute("SET threads = 1")
        ref, d1 = timed(lambda: duck.execute(sql).fetchall(), runs)
        duck.execute(f"SET threads = {os.cpu_count()}")
        _, dn = timed(lambda: duck.execute(sql).fetchall(), runs)
        lsql = sqlite_sql(sql)
        # first run with a time cap; a query that does not finish is reported, not timed
        deadline = time.perf_counter() + SQLITE_CAP_S
        lite.set_progress_handler(lambda: 1 if time.perf_counter() > deadline else 0, 100_000)
        try:
            t = time.perf_counter()
            lrows = lite.execute(lsql).fetchall()
            first = (time.perf_counter() - t) * 1000
        except sqlite3.OperationalError:
            lrows, first = None, None
        lite.set_progress_handler(None, 0)
        if lrows is None:
            ls, ok = None, None
        elif first > 5000:
            ls, ok = first, same(ref, lrows)  # slow queries: one run is enough
        else:
            lrows, ls = timed(lambda: lite.execute(lsql).fetchall(), runs)
            ok = same(ref, lrows)
        shown = f"{ls:9.1f} ms" if ls is not None else f"> {SQLITE_CAP_S} s"
        print(f"q{q:02d} duckdb_1t {d1:8.1f} ms  duckdb_{os.cpu_count()}t {dn:8.1f} ms  sqlite {shown}  sqlite_matches={ok}", flush=True)
        result["queries"].append({"q": q, "duckdb_1t_ms": round(d1, 2), "duckdb_mt_ms": round(dn, 2),
                                  "sqlite_ms": round(ls, 2) if ls is not None else None,
                                  "sqlite_matches_duckdb": ok})
    out = ROOT / "results" / f"bench_others_sf{sf}.json"
    out.parent.mkdir(exist_ok=True)
    out.write_text(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
