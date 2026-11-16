//! criterion benchmarks: the 22 TPC-H queries at SF 0.01 (data/sf0.01).
//! Run: cargo bench -p quackless-core   (QUACKLESS_SF=0.1 for a bigger scale)

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use quackless_core::{Database, Options};
use std::hint::black_box;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(sf: &str) -> Database {
    let mut db = Database::new();
    let ddl = std::fs::read_to_string(root().join("data/schema.sql")).expect("data/schema.sql (run scripts/gen_tpch.py)");
    db.execute_script(&ddl, Options::default()).unwrap();
    for (name, _, _) in db.tables() {
        let bytes = std::fs::read(root().join(format!("data/sf{}/{}.csv", sf, name))).unwrap();
        db.load_csv(&name, &bytes).unwrap();
    }
    db
}

fn tpch(c: &mut Criterion) {
    let sf = std::env::var("QUACKLESS_SF").unwrap_or_else(|_| "0.01".into());
    let db = load(&sf);
    let mut group = c.benchmark_group(format!("tpch_sf{}", sf));
    group.sample_size(20);
    for q in 1..=22 {
        let sql = std::fs::read_to_string(root().join(format!("queries/tpch/q{:02}.sql", q))).unwrap();
        let sql = sql.trim().trim_end_matches(';').to_string();
        group.bench_with_input(BenchmarkId::new("query", format!("q{:02}", q)), &sql, |b, sql| {
            b.iter(|| black_box(db.query(sql, Options::default()).unwrap().rows))
        });
    }
    group.finish();
}

criterion_group!(benches, tpch);
criterion_main!(benches);
