//! TPC-H differential run: every query in both engines, compared row for row.

use crate::common::*;
use crate::compare::compare;
use crate::oracle::Oracle;
use quackless_core::Options;
use std::time::Instant;

pub struct QueryOutcome {
    pub q: usize,
    pub status: &'static str,
    pub message: String,
    pub rows: usize,
    pub ours_ms: f64,
    pub duck_ms: f64,
}

pub fn run(sf: &str, only: &[usize], verbose: bool) -> Result<Vec<QueryOutcome>, String> {
    let root = root();
    let dir = format!("data/sf{}", sf);
    let t = Instant::now();
    let db = load_tpch(&root.join(&dir))?;
    eprintln!("sf {}: Quackless loaded in {:.1} s", sf, t.elapsed().as_secs_f64());
    let mut oracle = Oracle::spawn(&root)?;
    oracle.load_tpch(&dir)?;
    let mut out = Vec::new();
    for q in 1..=22 {
        if !only.is_empty() && !only.contains(&q) {
            continue;
        }
        let path = root.join(format!("queries/tpch/q{:02}.sql", q));
        let sql = strip(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?);
        let t0 = Instant::now();
        let duck = oracle.query(&sql);
        let duck_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = Instant::now();
        let ours = db.query(&sql, Options::default());
        let ours_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let (status, message, rows) = match (ours, duck) {
            (_, Err(e)) => ("oracle_error", e, 0),
            (Err(e), _) => ("error", e, 0),
            (Ok(r), Ok(d)) => match compare(&ours_cells(&r), &duck_cells(&d.rows), is_ordered(&sql)) {
                Ok(()) => ("pass", String::new(), r.rows),
                Err(e) => ("mismatch", e, r.rows),
            },
        };
        if verbose || status != "pass" {
            eprintln!("  q{:02} {:<9} {:>8.1} ms (DuckDB {:>7.1} ms) {}", q, status, ours_ms, duck_ms, message.chars().take(300).collect::<String>());
        } else {
            eprintln!("  q{:02} pass      {:>8.1} ms (DuckDB {:>7.1} ms)", q, ours_ms, duck_ms);
        }
        out.push(QueryOutcome { q, status, message, rows, ours_ms, duck_ms });
    }
    Ok(out)
}
