"""Data for the SQL reference and fuzzer casebook pages.

Usage: .venv/bin/python -I scripts/export_pages.py   (after `make build`)

- web/src/data/reference.json: every example on the SQL page, run through the
  release CLI over the SF 0.01 sample. The output shown on the page is the
  engine's own output; an example that fails here fails the script, unless it
  is listed as unsupported, in which case the engine's error is shown.
- web/src/data/casebook.json: the saved fuzzer regressions (tests/fuzz/*.sql),
  the open cases (tests/fuzz/open/*.sql) and the per-seed results.
"""
import glob
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
CLI = ROOT / "target" / "release" / "quackless"
DATA = ROOT / "data" / "sf0.01"

# (section, intro, [(label, sql)])
SECTIONS = [
    ("Selecting and filtering", "Column lists, aliases, WHERE with comparison, BETWEEN, IN lists and LIKE, ORDER BY with LIMIT and OFFSET.", [
        ("Filters", "SELECT count(*) AS parts\nFROM part\nWHERE p_size BETWEEN 10 AND 12\n  AND p_brand IN ('Brand#11', 'Brand#12')\n  AND p_type NOT LIKE '%BRASS'"),
        ("Top N with an offset", "SELECT o_orderkey, o_totalprice\nFROM orders\nORDER BY o_totalprice DESC\nLIMIT 3 OFFSET 2"),
    ]),
    ("Joins", "INNER, LEFT and CROSS joins with ON, and comma joins with the condition in WHERE. The optimizer turns equality conditions into hash join keys and picks the order.", [
        ("Explicit join", "SELECT n_name, r_name\nFROM nation\nJOIN region ON n_regionkey = r_regionkey\nORDER BY n_name\nLIMIT 3"),
        ("Left join that keeps unmatched rows", "SELECT c_mktsegment, count(o_orderkey) AS orders\nFROM customer\nLEFT JOIN orders ON o_custkey = c_custkey AND o_orderpriority = '1-URGENT'\nGROUP BY c_mktsegment\nORDER BY c_mktsegment"),
    ]),
    ("Grouping and aggregates", "count(*), count(x), count(DISTINCT x), sum, avg, min and max, with GROUP BY and HAVING. Grouping by an output position or alias works.", [
        ("HAVING and count(DISTINCT)", "SELECT p_brand, count(*) AS parts, count(DISTINCT p_size) AS sizes\nFROM part\nGROUP BY p_brand\nHAVING count(*) > 70\nORDER BY p_brand\nLIMIT 3"),
        ("Group by a CASE expression", "SELECT CASE WHEN o_totalprice > 300000 THEN 'big'\n            WHEN o_totalprice > 100000 THEN 'mid'\n            ELSE 'small' END AS size,\n       count(*) AS orders\nFROM orders\nGROUP BY 1\nORDER BY 2 DESC"),
    ]),
    ("Subqueries and CTEs", "EXISTS, NOT EXISTS, IN and NOT IN subqueries, scalar subqueries (correlated or not), derived tables and WITH. The binder rewrites correlated subqueries into joins.", [
        ("Correlated EXISTS", "SELECT n_name\nFROM nation n\nWHERE EXISTS (\n  SELECT 1 FROM supplier s\n  WHERE s.s_nationkey = n.n_nationkey AND s_acctbal > 9900\n)\nORDER BY n_name"),
        ("Correlated scalar subquery", "SELECT c_name,\n       (SELECT count(*) FROM orders WHERE o_custkey = c_custkey) AS orders\nFROM customer\nORDER BY orders DESC, c_name\nLIMIT 3"),
        ("WITH", "WITH per_customer AS (\n  SELECT o_custkey, count(*) AS n FROM orders GROUP BY o_custkey\n)\nSELECT max(n) AS most_orders, round(avg(n), 2) AS average\nFROM per_customer"),
    ]),
    ("Set operations", "UNION, UNION ALL, EXCEPT and INTERSECT, with ORDER BY and LIMIT on the result.", [
        ("UNION ALL", "SELECT r_name AS name FROM region\nUNION ALL\nSELECT 'ATLANTIS'\nORDER BY name\nLIMIT 3"),
        ("EXCEPT", "SELECT n_regionkey FROM nation\nEXCEPT\nSELECT r_regionkey FROM region WHERE r_name = 'ASIA'\nORDER BY 1"),
    ]),
    ("Arithmetic and NULL", "Integer division returns a double and division by zero returns infinity, as in DuckDB. NULL propagates through arithmetic; IS [NOT] DISTINCT FROM compares NULL safely.", [
        ("Division", "SELECT 7 / 2 AS half, 7 % 3 AS remainder, 1 / 0 AS by_zero"),
        ("NULL handling", "SELECT 1 + NULL AS plus_null,\n       coalesce(NULL, 2) AS first_value,\n       nullif(3, 3) AS null_if_equal,\n       NULL IS NOT DISTINCT FROM NULL AS same"),
        ("Casts", "SELECT CAST(3.7 AS INTEGER) AS i,\n       CAST(42 AS VARCHAR) || '!' AS s,\n       CAST('12' AS DOUBLE) AS d"),
    ]),
    ("Dates", "DATE literals, INTERVAL arithmetic in days, months and years, date differences in days, EXTRACT and the matching functions.", [
        ("Interval arithmetic", "SELECT DATE '1998-12-01' - INTERVAL '90' DAY AS cutoff,\n       DATE '2024-02-29' + INTERVAL 1 YEAR AS next_year,\n       DATE '1995-03-01' - DATE '1995-01-01' AS days"),
        ("Parts of a date", "SELECT EXTRACT(YEAR FROM DATE '1995-06-17') AS y,\n       month(DATE '1995-06-17') AS m,\n       quarter(DATE '1995-06-17') AS q,\n       dayofweek(DATE '1995-06-17') AS dow"),
    ]),
    ("Strings and numbers", "String functions: || and concat, length, substring, upper, lower, trim, ltrim, rtrim, replace, reverse, starts_with, ends_with, contains, strpos. Numeric: abs, round, ceil, floor, sign, sqrt, power, ln, log10, exp, greatest, least.", [
        ("Strings", "SELECT 'quack' || 'less' AS joined,\n       substring('quackless', 1, 5) AS sub,\n       upper(trim('  duck ')) AS cleaned,\n       replace('a-b-c', '-', '+') AS replaced,\n       starts_with('duckdb', 'duck') AS prefix"),
        ("Numbers", "SELECT round(3.14159, 2) AS r,\n       power(2, 10) AS p,\n       sqrt(2.0) AS root,\n       greatest(1, 5, 3) AS g,\n       floor(-1.2) AS f"),
    ]),
]

UNSUPPORTED = [
    ("Window functions", "SELECT n_name, row_number() OVER (ORDER BY n_name) FROM nation"),
    ("UPDATE and DELETE", "UPDATE region SET r_name = 'X'"),
    ("VALUES as a table", "SELECT * FROM (VALUES (1), (2)) AS t(x)"),
    ("string_agg and other aggregates", "SELECT string_agg(r_name, ', ') FROM region"),
    ("Timestamps", "SELECT now()"),
]


def run(sql):
    p = subprocess.run([str(CLI), "--data", str(DATA), "-c", sql], capture_output=True, text=True, timeout=60)
    out = [l for l in (p.stdout + p.stderr).splitlines() if not l.startswith("loaded ")]
    text = "\n".join(l.rstrip() for l in out)
    err = next((l for l in out if l.startswith("Error:")), None)
    # drop the timing footer, keep the row count
    text = re.sub(r"\n\((\d+) rows?; [^)]*\)$", lambda m: f"\n({m.group(1)} row{'s' if m.group(1) != '1' else ''})", text)
    return text, err


def reference():
    sections = []
    for title, intro, examples in SECTIONS:
        ex = []
        for label, sql in examples:
            text, err = run(sql)
            if err:
                raise SystemExit(f"example failed: {label}: {err}")
            ex.append({"label": label, "sql": sql, "output": text})
        sections.append({"title": title, "intro": intro, "examples": ex})
    unsupported = []
    for label, sql in UNSUPPORTED:
        text, err = run(sql)
        if not err:
            raise SystemExit(f"listed as unsupported but runs: {label}")
        unsupported.append({"label": label, "sql": sql, "error": err.removeprefix("Error: ")})
    return {"sections": sections, "unsupported": unsupported}


def parse_case(path):
    t = pathlib.Path(path).read_text()
    head, rest = t.split("-- query\n", 1)
    query, expected = rest.split("-- expected\n", 1)
    setup = "\n".join(l for l in head.splitlines() if not l.startswith("--")).strip()
    rows = {}
    for m in re.finditer(r"INSERT INTO (\w+) VALUES (.*);", setup):
        rows[m.group(1)] = m.group(2).count("), (") + 1
    return {
        "id": pathlib.Path(path).stem,
        "setup": setup,
        "query": query.strip().rstrip(";"),
        "expected": json.loads(expected.strip() or "[]"),
        "rows": rows,
    }


def tags(q):
    found = []
    checks = [
        ("division", r"/"), ("modulo", r"%"), ("CASE", r"\bCASE\b"), ("COALESCE", r"\bcoalesce\b"),
        ("NULLIF", r"\bnullif\b"), ("IS DISTINCT FROM", r"DISTINCT FROM"), ("subquery", r"\(SELECT"),
        ("join", r"\bJOIN\b|FROM t1, t2"), ("GROUP BY", r"GROUP BY"), ("ORDER BY", r"ORDER BY"),
        ("min / max", r"\b(min|max)\("), ("set operation", r"\b(UNION|EXCEPT|INTERSECT)\b"),
        ("NULL literal", r"\bNULL\b"),
    ]
    for name, rx in checks:
        if re.search(rx, q, re.I):
            found.append(name)
    return found


def casebook():
    fixed = [parse_case(p) for p in sorted(glob.glob(str(ROOT / "tests" / "fuzz" / "*.sql")))]
    open_ = [parse_case(p) for p in sorted(glob.glob(str(ROOT / "tests" / "fuzz" / "open" / "*.sql")))]
    for c in fixed + open_:
        c["tags"] = tags(c["query"])
    for c in open_:
        c["kind"] = "two-level correlation" if c["query"].count("(SELECT") >= 2 else ("x % 0" if "%" in c["query"] else "other")
    runs = [json.loads(pathlib.Path(p).read_text()) for p in glob.glob(str(ROOT / "results" / "fuzz_seed*.json"))]
    seeds = sorted(({k: r[k] for k in ("seed", "queries", "agree", "both_error", "rejected_by_duckdb", "seconds")} | {"mismatches": len(r["mismatches"])} for r in runs), key=lambda r: r["seed"])
    return {"fixed": fixed, "open": open_, "seeds": seeds}


def main():
    out = ROOT / "web" / "src" / "data"
    (out / "reference.json").write_text(json.dumps(reference(), indent=1))
    (out / "casebook.json").write_text(json.dumps(casebook(), indent=1))
    print("wrote reference.json and casebook.json")


if __name__ == "__main__":
    main()
