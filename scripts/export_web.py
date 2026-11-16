"""Collect everything the site needs into web/.

Usage: .venv/bin/python -I scripts/export_web.py

- web/public/data/: schema.sql and the SF 0.01 CSVs the browser engine loads
- web/public/quackless_wasm_bg.wasm: the engine (built by `make wasm`)
- web/src/data/queries.json: the 22 TPC-H queries plus a few starters
- web/src/data/conformance.json: TPC-H, sqllogictest, fuzzer and benchmark
  results, merged from results/*.json and tests/fuzz/
"""
import datetime
import glob
import json
import pathlib
import shutil

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web"

STARTERS = [
    ("Filter and aggregate", "SELECT l_returnflag, count(*) AS line_items, sum(l_quantity) AS units\nFROM lineitem\nWHERE l_shipdate <= DATE '1998-09-02'\nGROUP BY l_returnflag\nORDER BY l_returnflag"),
    ("Join and rank", "SELECT n_name, count(*) AS customers, round(avg(c_acctbal), 2) AS avg_balance\nFROM customer\nJOIN nation ON c_nationkey = n_nationkey\nGROUP BY n_name\nORDER BY customers DESC\nLIMIT 10"),
    ("Pushdown to see", "SELECT o_orderpriority, count(*) AS orders\nFROM orders, customer\nWHERE o_custkey = c_custkey\n  AND c_mktsegment = 'BUILDING'\n  AND o_orderdate >= DATE '1995-01-01' + INTERVAL 1 MONTH\nGROUP BY o_orderpriority\nORDER BY o_orderpriority"),
]

TPCH_TITLES = {
    1: "Pricing summary report", 2: "Minimum cost supplier", 3: "Shipping priority", 4: "Order priority checking",
    5: "Local supplier volume", 6: "Forecasting revenue change", 7: "Volume shipping", 8: "National market share",
    9: "Product type profit measure", 10: "Returned item reporting", 11: "Important stock identification",
    12: "Shipping modes and order priority", 13: "Customer distribution", 14: "Promotion effect",
    15: "Top supplier", 16: "Parts/supplier relationship", 17: "Small-quantity-order revenue",
    18: "Large volume customer", 19: "Discounted revenue", 20: "Potential part promotion",
    21: "Suppliers who kept orders waiting", 22: "Global sales opportunity",
}


def load(name):
    p = ROOT / "results" / name
    return json.loads(p.read_text()) if p.exists() else None


def slt_summary():
    d = load("slt.json")
    if not d:
        return None
    groups = {}
    cats = {}
    for f in d["files"]:
        name = f["file"]
        g = "select1 to select5" if name.startswith("select") else ("evidence" if name.startswith("evidence") else "random/" + name.split("/")[1])
        a = groups.setdefault(g, {"group": g, "files": 0, "records": 0, "passed": 0})
        a["files"] += 1
        a["records"] += f["records"]
        a["passed"] += f["passed"]
        for c, n in f.get("failure_categories", {}).items():
            cats[c] = cats.get(c, 0) + n
    order = ["select1 to select5", "evidence", "random/aggregates", "random/expr", "random/groupby", "random/select"]
    return {
        "records": d["records"],
        "passed": d["passed"],
        "files": len(d["files"]),
        "groups": sorted(groups.values(), key=lambda g: order.index(g["group"]) if g["group"] in order else 99),
        "failure_categories": sorted(({"category": c, "records": n} for c, n in cats.items()), key=lambda x: -x["records"]),
    }


def fuzz_summary():
    runs = [json.loads(pathlib.Path(p).read_text()) for p in sorted(glob.glob(str(ROOT / "results" / "fuzz_seed*.json")))]
    fixed = sorted(glob.glob(str(ROOT / "tests" / "fuzz" / "*.sql")))
    open_ = sorted(glob.glob(str(ROOT / "tests" / "fuzz" / "open" / "*.sql")))

    def query_of(p):
        t = pathlib.Path(p).read_text()
        return t.split("-- query\n", 1)[1].split("-- expected", 1)[0].strip().rstrip(";")

    def open_kind(q):
        if q.count("(SELECT") >= 2:
            return "correlated subquery two levels deep"
        if "%" in q:
            return "x % 0: depends on which rows DuckDB's plan evaluates"
        return "other"

    kinds = {}
    for p in open_:
        k = open_kind(query_of(p))
        kinds[k] = kinds.get(k, 0) + 1
    return {
        "seeds": [r["seed"] for r in runs],
        "queries": sum(r["queries"] for r in runs),
        "agree": sum(r["agree"] for r in runs),
        "both_error": sum(r["both_error"] for r in runs),
        "rejected_by_duckdb": sum(r["rejected_by_duckdb"] for r in runs),
        "mismatches": sum(len(r["mismatches"]) for r in runs),
        "fixed_regressions": len(fixed),
        "fixed_examples": [query_of(p) for p in fixed[:4]],
        "open": len(open_),
        "open_kinds": [{"kind": k, "count": n} for k, n in sorted(kinds.items(), key=lambda x: -x[1])],
    }


def bench_summary(sf="0.1"):
    q = load(f"bench_quackless_sf{sf}.json")
    o = load(f"bench_others_sf{sf}.json")
    if not q or not o:
        return None
    rows = []
    for a, b in zip(q["queries"], o["queries"]):
        rows.append({"q": a["q"], "quackless_ms": a["quackless_ms"], "duckdb_1t_ms": b["duckdb_1t_ms"],
                     "duckdb_mt_ms": b["duckdb_mt_ms"], "sqlite_ms": b["sqlite_ms"],
                     "sqlite_matches": b["sqlite_matches_duckdb"]})
    return {"sf": sf, "runs": q["runs"], "cpu_count": o["cpu_count"], "duckdb_version": o["duckdb_version"],
            "sqlite_version": o["sqlite_version"], "sqlite_cap_s": o.get("sqlite_cap_s"), "queries": rows}


def main():
    pub = WEB / "public" / "data"
    pub.mkdir(parents=True, exist_ok=True)
    shutil.copy(ROOT / "data" / "schema.sql", pub / "schema.sql")
    for csv in (ROOT / "data" / "sf0.01").glob("*.csv"):
        shutil.copy(csv, pub / csv.name)
    wasm = WEB / "src" / "wasm-pkg" / "quackless_wasm_bg.wasm"
    if wasm.exists():
        shutil.copy(wasm, WEB / "public" / "quackless_wasm_bg.wasm")

    out = WEB / "src" / "data"
    out.mkdir(parents=True, exist_ok=True)
    queries = [{"id": f"s{i + 1}", "group": "Starters", "title": t, "sql": s} for i, (t, s) in enumerate(STARTERS)]
    for n in range(1, 23):
        sql = (ROOT / "queries" / "tpch" / f"q{n:02d}.sql").read_text().strip().rstrip(";")
        queries.append({"id": f"q{n}", "group": "TPC-H", "title": f"Q{n}. {TPCH_TITLES[n]}", "sql": sql})
    (out / "queries.json").write_text(json.dumps(queries, indent=1))

    tables = {p.stem: sum(1 for _ in p.open()) - 1 for p in (ROOT / "data" / "sf0.01").glob("*.csv")}
    conf = {
        "generated": datetime.date.today().isoformat(),
        "sample_rows": tables,
        "tpch": load("tpch.json"),
        "slt": slt_summary(),
        "fuzz": fuzz_summary(),
        "bench": bench_summary(),
    }
    (out / "conformance.json").write_text(json.dumps(conf, indent=1))
    print("wrote", out / "conformance.json", "and", out / "queries.json")


if __name__ == "__main__":
    main()
