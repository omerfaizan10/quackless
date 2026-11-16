//! quackless-harness: differential testing and benchmarks.
//!
//!   tpch [--sf 0.01,0.1,1] [--q 1,6]   TPC-H against DuckDB, writes results/tpch.json
//!   fuzz [--seed 1] [--n 2000]          differential fuzzer, writes results/fuzz.json and tests/fuzz/*.sql
//!   bench [--sf 0.1] [--runs 5]         median time per TPC-H query, writes results/bench_quackless_sf<SF>.json
//!   slt [PATH...]                      sqllogictest subset (default data/slt), writes results/slt.json

mod common;
mod compare;
mod fuzz;
mod oracle;
mod slt;
mod tpch;

use common::{json_str, root};
use std::fmt::Write;

fn list<T: std::str::FromStr>(s: &str) -> Vec<T> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let res = match cmd {
        "tpch" => cmd_tpch(flag("--sf").unwrap_or("0.01".into()), flag("--q").map(|s| list(&s)).unwrap_or_default(), args.iter().any(|a| a == "-v")),
        "fuzz" => cmd_fuzz(flag("--seed").and_then(|s| s.parse().ok()).unwrap_or(1), flag("--n").and_then(|s| s.parse().ok()).unwrap_or(2000)),
        "bench" => cmd_bench(flag("--sf").unwrap_or("0.1".into()), flag("--runs").and_then(|s| s.parse().ok()).unwrap_or(5)),
        "fuzz-triage" => fuzz::triage().map(|(f, o)| eprintln!("{} fixed (moved to tests/fuzz), {} still open", f, o)),
        "slt" => cmd_slt(&args[1..]),
        "slt-child" => slt::child(std::path::Path::new(&args[1]), args[2].parse().unwrap_or(-1)),
        _ => Err("usage: quackless-harness tpch [--sf 0.01,0.1,1] [--q 1,6]".into()),
    };
    if let Err(e) = res {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn cmd_tpch(sfs: String, only: Vec<usize>, verbose: bool) -> Result<(), String> {
    let mut j = String::from("{\"scale_factors\":[");
    let mut first = true;
    for sf in sfs.split(',') {
        let res = tpch::run(sf.trim(), &only, verbose)?;
        let pass = res.iter().filter(|r| r.status == "pass").count();
        eprintln!("sf {}: {} of {} pass", sf, pass, res.len());
        if !first {
            j.push(',');
        }
        first = false;
        let _ = write!(j, "{{\"sf\":{},\"passed\":{},\"total\":{},\"queries\":[", json_str(sf.trim()), pass, res.len());
        for (i, r) in res.iter().enumerate() {
            if i > 0 {
                j.push(',');
            }
            let _ = write!(
                j,
                "{{\"q\":{},\"status\":{},\"message\":{},\"rows\":{},\"quackless_ms\":{:.2},\"duckdb_ms\":{:.2}}}",
                r.q,
                json_str(r.status),
                json_str(&r.message),
                r.rows,
                r.ours_ms,
                r.duck_ms
            );
        }
        j.push_str("]}");
    }
    j.push_str("]}");
    if only.is_empty() {
        let dir = root().join("results");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("tpch.json"), j).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn slt_files(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if p.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(p).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
        v.sort();
        for x in v {
            slt_files(&x, out);
        }
    } else if p.extension().map(|e| e == "test").unwrap_or(false) {
        out.push(p.to_path_buf());
    }
}

/// Coarse reason for a failed sqllogictest record.
fn slt_category(why: &str) -> &'static str {
    if why.starts_with("got [") || why.contains("hashing to") || why.contains("columns, expected") {
        "wrong result"
    } else if why.contains("must appear in the GROUP BY") {
        "non-grouped column (SQLite allows it)"
    } else if why.starts_with("timeout") {
        "timeout"
    } else if why.starts_with("Parser error") {
        "parse error (unsupported syntax)"
    } else if why == "expected an error" {
        "accepted a statement SQLite rejects"
    } else {
        "other error"
    }
}

fn cmd_slt(args: &[String]) -> Result<(), String> {
    let root = root();
    let base = root.join("data/slt");
    let paths: Vec<std::path::PathBuf> = if args.is_empty() { vec![base.clone()] } else { args.iter().map(std::path::PathBuf::from).collect() };
    let mut files = Vec::new();
    for p in &paths {
        slt_files(p, &mut files);
    }
    let (mut total, mut passed) = (0usize, 0usize);
    let mut j = String::from("{\"files\":[");
    for (k, f) in files.iter().enumerate() {
        let o = slt::run_file(f, std::time::Duration::from_secs(10))?;
        total += o.total;
        passed += o.passed;
        let name = f.strip_prefix(&base).unwrap_or(f).display().to_string();
        eprintln!("{:<44} {:>6} / {:<6} ({:.1}%){}", name, o.passed, o.total, 100.0 * o.passed as f64 / o.total.max(1) as f64, if o.timeouts > 0 { format!(", {} timeouts", o.timeouts) } else { String::new() });
        if k > 0 {
            j.push(',');
        }
        let _ = write!(j, "{{\"file\":{},\"records\":{},\"passed\":{},\"statements\":{},\"statements_passed\":{},\"timeouts\":{},\"failure_categories\":", json_str(&name), o.total, o.passed, o.statements, o.statements_passed, o.timeouts);
        let mut cats: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for (_, _, why) in &o.failures {
            *cats.entry(slt_category(why)).or_default() += 1;
        }
        j.push_str("{");
        for (i, (c, n)) in cats.iter().enumerate() {
            if i > 0 {
                j.push(',');
            }
            let _ = write!(j, "{}:{}", json_str(c), n);
        }
        j.push_str("},\"sample_failures\":[");
        for (i, (line, sql, why)) in o.failures.iter().take(5).enumerate() {
            if i > 0 {
                j.push(',');
            }
            let _ = write!(j, "{{\"line\":{},\"sql\":{},\"reason\":{}}}", line, json_str(sql), json_str(why));
        }
        j.push_str("]}");
    }
    let _ = write!(j, "],\"records\":{},\"passed\":{}}}", total, passed);
    eprintln!("sqllogictest: {} of {} records pass ({:.2}%)", passed, total, 100.0 * passed as f64 / total.max(1) as f64);
    if args.is_empty() {
        std::fs::create_dir_all(root.join("results")).map_err(|e| e.to_string())?;
        std::fs::write(root.join("results/slt.json"), j).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cmd_fuzz(seed: u64, n: usize) -> Result<(), String> {
    let t = std::time::Instant::now();
    let st = fuzz::run(seed, n)?;
    let mut oracle = oracle::Oracle::spawn(&root())?;
    let mut saved = Vec::new();
    for (k, (sql, _, setup)) in st.mismatches.iter().enumerate() {
        let p = fuzz::save_regression(&mut oracle, &format!("seed{}_{:02}", seed, k + 1), setup, sql)?;
        saved.push(p.file_name().unwrap().to_string_lossy().to_string());
    }
    let secs = t.elapsed().as_secs_f64();
    eprintln!(
        "fuzz seed {}: {} queries in {:.0} s: {} agree, {} both error, {} rejected by DuckDB, {} minimized mismatches",
        seed, st.queries, secs, st.agree, st.both_error, st.invalid, st.mismatches.len()
    );
    let mut j = format!(
        "{{\"seed\":{},\"queries\":{},\"agree\":{},\"both_error\":{},\"rejected_by_duckdb\":{},\"seconds\":{:.1},\"mismatches\":[",
        seed, st.queries, st.agree, st.both_error, st.invalid, secs
    );
    for (k, (sql, why, _)) in st.mismatches.iter().enumerate() {
        if k > 0 {
            j.push(',');
        }
        let _ = write!(j, "{{\"sql\":{},\"difference\":{},\"file\":{}}}", json_str(sql), json_str(why), json_str(&saved[k]));
    }
    j.push_str("]}");
    std::fs::create_dir_all(root().join("results")).map_err(|e| e.to_string())?;
    std::fs::write(root().join(format!("results/fuzz_seed{}.json", seed)), j).map_err(|e| e.to_string())?;
    Ok(())
}

fn cmd_bench(sf: String, runs: usize) -> Result<(), String> {
    let db = common::load_tpch(&root().join(format!("data/sf{}", sf)))?;
    let mut j = format!("{{\"sf\":{},\"runs\":{},\"queries\":[", json_str(&sf), runs);
    for q in 1..=22 {
        let sql = common::strip(&std::fs::read_to_string(root().join(format!("queries/tpch/q{:02}.sql", q))).map_err(|e| e.to_string())?);
        let mut ts = Vec::new();
        for _ in 0..runs {
            let t = std::time::Instant::now();
            db.query(&sql, quackless_core::Options::default())?;
            ts.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = ts[ts.len() / 2];
        eprintln!("q{:02} {:8.2} ms", q, med);
        if q > 1 {
            j.push(',');
        }
        let _ = write!(j, "{{\"q\":{},\"quackless_ms\":{:.2}}}", q, med);
    }
    j.push_str("]}");
    std::fs::write(root().join(format!("results/bench_quackless_sf{}.json", sf)), j).map_err(|e| e.to_string())?;
    Ok(())
}
