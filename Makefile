# Quackless: every number on the site and in the README comes from `make eval`.
export PATH := /opt/homebrew/opt/rustup/bin:$(HOME)/.cargo/bin:$(PATH)
PY := .venv/bin/python -I
H := ./target/release/quackless-harness

.PHONY: data build test wasm tpch slt fuzz bench eval web

data:            ## TPC-H at SF 0.01, 0.1, 1 (DuckDB tpch extension) and the sqllogictest subset
	$(PY) scripts/gen_tpch.py 0.01 0.1 1
	$(PY) scripts/fetch_slt.py

build:
	cargo build --release

test: build      ## unit tests plus every saved fuzzer regression
	cargo test --release

wasm:            ## engine compiled to WebAssembly for the workbench
	cd crates/wasm && wasm-pack build --release --target web --out-dir ../../web/src/wasm-pkg --no-pack && rm -f ../../web/src/wasm-pkg/.gitignore

tpch: build      ## 22 queries x 3 scale factors against DuckDB -> results/tpch.json
	$(H) tpch --sf 0.01,0.1,1

slt: build       ## sqllogictest subset -> results/slt.json
	$(H) slt

fuzz: build      ## 10 seeds x 5,000 random queries against DuckDB -> results/fuzz_seed*.json
	for s in 1 2 3 4 5 6 7 8 9 10; do $(H) fuzz --seed $$s --n 5000 || exit 1; done
	$(H) fuzz-triage

bench: build     ## median of 5 runs per TPC-H query at SF 0.1: Quackless, DuckDB, SQLite
	$(H) bench --sf 0.1 --runs 5
	$(PY) scripts/bench_others.py 0.1 5

eval: test tpch slt fuzz bench web

web: wasm        ## copy data, engine and results into web/
	$(PY) scripts/export_web.py
