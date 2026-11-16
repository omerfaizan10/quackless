"""Generate TPC-H data with DuckDB's tpch extension and export it as CSV.

Usage: .venv/bin/python scripts/gen_tpch.py 0.01 0.1 1

Writes data/sf<SF>/<table>.csv (with header), data/schema.sql (DuckDB DDL of
the generated tables) and queries/tpch/qNN.sql (the 22 queries as shipped by
the tpch extension).
"""
import pathlib
import sys

import duckdb

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLES = ["region", "nation", "supplier", "customer", "part", "partsupp", "orders", "lineitem"]


def main(sfs):
    for sf in sfs:
        out = ROOT / "data" / f"sf{sf}"
        out.mkdir(parents=True, exist_ok=True)
        con = duckdb.connect()
        con.sql("INSTALL tpch; LOAD tpch")
        con.sql(f"CALL dbgen(sf={sf})")
        for t in TABLES:
            con.sql(f"COPY {t} TO '{out / (t + '.csv')}' (HEADER, DELIMITER ',')")
        if sf == sfs[0]:
            ddl = [r[0].rstrip(";") + ";" for r in con.sql(
                "SELECT sql FROM duckdb_tables() ORDER BY table_name").fetchall()]
            (ROOT / "data" / "schema.sql").write_text("\n".join(ddl) + "\n")
            qdir = ROOT / "queries" / "tpch"
            qdir.mkdir(parents=True, exist_ok=True)
            for nr, q in con.sql("SELECT query_nr, query FROM tpch_queries()").fetchall():
                (qdir / f"q{nr:02d}.sql").write_text(q)
        counts = {t: con.sql(f"SELECT count(*) FROM {t}").fetchall()[0][0] for t in TABLES}
        print(f"sf={sf}", counts)


if __name__ == "__main__":
    main(sys.argv[1:] or ["0.01"])
