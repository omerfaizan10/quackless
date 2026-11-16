# Quackless

A columnar, vectorized SQL query engine I wrote from scratch in Rust. It compiles to WebAssembly and runs in the browser, and I check every answer it gives against DuckDB.

**Live workbench:** https://quackless-sql.vercel.app (TPC-H sample loaded in the page, with the query plan replayed operator by operator)

## The site

- **Workbench** (`/`): write SQL or pick a TPC-H query, then:
  - watch the physical plan replay operator by operator
  - compare the logical plan before and after the optimizer
  - see how many rows each operator produced
  - browse the tables loaded in the page
- **SQL** (`/sql/`): the dialect by example. Each example's output is the engine's own, captured at build time by `scripts/export_pages.py`.
- **Conformance** (`/conformance/`): the TPC-H, sqllogictest, fuzzing and benchmark results below, in more detail.
- **Fuzzer** (`/fuzzer/`): every minimized case the fuzzer saved, with a button that re-runs it in your browser and compares the answer with DuckDB's.
- **Internals** (`/internals/`): how a query moves through the engine.

The engine runs in a Web Worker, so a slow query never freezes the page, and the Stop button can kill it. In the browser, nested-loop joins may compare at most 10 million row pairs per query. With the optimizer switched off, every join is a nested loop (hash join keys come from the optimizer), so most multi-table TPC-H queries stop with an explanation instead of running for minutes.

## Results

All numbers come from `make eval` and are stored in `results/`.

| Check | Result |
|---|---|
| TPC-H, 22 queries, compared row by row with DuckDB 2.0.0 | **22 of 22 match at scale factors 0.01, 0.1 and 1** |
| SQLite sqllogictest, 55-file subset run in SQLite mode | **443,383 of 444,795 records pass (99.68%)** |
| Differential fuzzer, 10 seeds × 5,000 random queries | **49,999 of 50,000 agree with DuckDB** (the other one DuckDB rejected); 53 earlier mismatches are now regression tests |
| Speed, TPC-H SF 0.1, median of 5 runs | 2.6× slower than one-thread DuckDB and 28× faster than SQLite without indexes (geometric means) |
| Speed, TPC-H SF 1 | 3.5 s for all 22 queries, against 0.21 s for DuckDB on all cores (about 16×) |

Quackless is slower than DuckDB, and I expected it to be. The point of the comparison is to show where the gap is.

## What I wrote myself

The core crate (`crates/core`, about 7,400 lines) has no SQL library in it. The only dependency is `csv`, for reading input.

- **Lexer and parser** (`lexer.rs`, `parser.rs`): hand-written recursive descent, with one function per precedence level.
- **Binder and logical planner** (`planner.rs`): resolves names and pulls out aggregates. It decorrelates subqueries:
  - `EXISTS` and `IN` become semi joins, and `NOT EXISTS` becomes an anti join.
  - A correlated scalar subquery becomes an aggregate joined back on the correlated columns.
  - Anything else runs once per distinct outer value, with the results cached.
- **Rule-based optimizer** (`optimizer.rs`):
  - constant folding
  - predicate pushdown, including OR factoring
  - join ordering, using output estimates from exact distinct counts of the key columns
  - projection pruning
- **Physical operators** (`exec.rs`, `eval.rs`): pull-based, over batches of 2,048 rows. Operators: scan, filter, projection, hash join (inner, left, semi, anti), hash aggregate, sort, limit, nested loop join, and set operations. CASE and COALESCE evaluate each branch only on the rows that reach it.

I used my own column vectors instead of arrow-rs. The engine only needs a handful of physical types, and leaving Arrow out keeps the WebAssembly module small: 727 KB.

The other crates:
- `crates/cli`: a REPL.
- `crates/wasm`: the browser bindings.
- `crates/harness`: TPC-H differential runs, the sqllogictest runner, the fuzzer, and benchmarks.

## How answers are compared

`crates/harness/src/compare.rs` has tests for each of these rules.

- NULL equals only NULL.
- Integers must match exactly. Strings and dates (as ISO text) must match byte for byte.
- Any other number is compared with a relative tolerance of 1e-9 (absolute 1e-9 near zero). I store `DECIMAL` as a 64-bit float, while DuckDB keeps exact decimals, so sums over millions of rows can differ in the last bits. Infinities must match exactly.
- Without an `ORDER BY` on the outer query, rows are compared as a multiset. With one, order matters.

DuckDB runs as a separate process (`scripts/duck_oracle.py`). It loads the same CSV files as Quackless.

## Bugs the checks found

- **Join order (found by TPC-H at SF 1).** The join orderer picked the next table by input size alone. In Q5 it joined lineitem to customer on nation key, about 1.2M × 6,000 rows, and never finished. It now picks the join with the smallest estimated output, and Q5 takes about 150 ms.
- **Constant folding in SQLite mode (found by sqllogictest).** Folding always used DuckDB's division rules. In SQLite mode, `57 / 36` became 1.58 at plan time but 1 at run time. Fixing it raised the pass rate from 99.10% to 99.68%.
- **Eager CASE and COALESCE (found by the fuzzer).** Every branch was evaluated on every row, so a branch no row reached could still raise a division-by-zero error.
- **Also from the fuzzer:**
  - NaN ordering in MIN, MAX and ORDER BY (DuckDB puts NaN above every number)
  - DuckDB's `1 / 0 = inf` and its error on `x % 0`
  - folding arithmetic on a NULL literal
  - missing `IS [NOT] DISTINCT FROM`

## Known limits

- Single-threaded and in memory: no indexes, no storage, no UPDATE or DELETE, no window functions, no timestamps.
- `DECIMAL` is a 64-bit float.
- Correlated subqueries can reference the query directly around them, not one two levels up. Subqueries inside aggregate arguments are not supported.
- Whether `x % 0` raises an error depends on which rows get evaluated. DuckDB orders filters differently, so the two engines can disagree on whether a query errors.
  - 45 saved fuzzer cases sit in `tests/fuzz/open/` for this reason: 23 for `% 0`, and 22 for correlation two levels deep.
- The remaining sqllogictest failures, by cause:

  | Cause | Records |
  |---|---|
  | Non-grouped columns in grouped queries. SQLite allows them; DuckDB and PostgreSQL do not. | 1,278 |
  | SQLite-only syntax: `x IN table`, empty `IN ()`, blob literals | 88 |
  | Wrong results, mostly SQLite typing values one at a time where I type expressions statically | 30 |
  | Other errors | 15 |

## Running it

You need Rust (with the `wasm32-unknown-unknown` target and `wasm-pack`), Python 3.12, and Node.

```bash
uv venv --python 3.12 && uv pip install duckdb
make data      # TPC-H CSVs at SF 0.01 / 0.1 / 1 via DuckDB's tpch extension, plus the sqllogictest subset
make eval      # tests, TPC-H, sqllogictest, fuzzer, benchmarks; writes results/ and the site's data
cargo run --release -p quackless-cli -- --data data/sf0.01      # REPL over the TPC-H sample
cd web && npm install && npm run dev                             # workbench at http://localhost:3105
```

`cargo bench -p quackless-core` runs the 22 TPC-H queries as criterion benchmarks.

## Data

- TPC-H data is generated locally with DuckDB's `tpch` extension.
- The sqllogictest files come from SQLite's sqllogictest repository, which is public domain. `scripts/fetch_slt.py` downloads them from a git mirror pinned to one commit.
