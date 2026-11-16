//! quackless [--data DIR] [-c SQL] [--explain]
//!
//! With --data, creates the TPC-H tables from DIR/../schema.sql and loads
//! DIR/<table>.csv. Without -c, reads statements from stdin (a REPL when
//! stdin is a terminal), one statement per `;`.

use quackless_core::{Database, Options, QueryResult};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn load_tpch(db: &mut Database, dir: &Path) -> Result<(), String> {
    let schema = dir.parent().unwrap_or(dir).join("schema.sql");
    let ddl = std::fs::read_to_string(&schema).map_err(|e| format!("{}: {}", schema.display(), e))?;
    db.execute_script(&ddl, Options::default())?;
    for (name, _, _) in db.tables() {
        let p = dir.join(format!("{}.csv", name));
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        db.load_csv(&name, &bytes)?;
    }
    Ok(())
}

fn print_result(r: &QueryResult, explain: bool) {
    if let Some(n) = r.affected {
        println!("OK ({} rows)", n);
        return;
    }
    if explain {
        if let (Some(b), Some(a)) = (&r.plan_before, &r.plan_after) {
            println!("-- logical plan\n{}-- optimized\n{}", b.display_tree(), a.display_tree());
        }
    }
    let rows = r.string_rows();
    let mut w: Vec<usize> = r.names.iter().map(|n| n.len()).collect();
    for row in rows.iter().take(200) {
        for (i, c) in row.iter().enumerate() {
            w[i] = w[i].max(c.len());
        }
    }
    let line = |cells: &[String]| cells.iter().enumerate().map(|(i, c)| format!("{:<width$}", c, width = w[i])).collect::<Vec<_>>().join(" | ");
    println!("{}", line(&r.names));
    println!("{}", w.iter().map(|n| "-".repeat(*n)).collect::<Vec<_>>().join("-+-"));
    for row in rows.iter().take(200) {
        println!("{}", line(row));
    }
    if r.rows > 200 {
        println!("... {} more rows", r.rows - 200);
    }
    println!("({} rows; plan {:.2} ms, optimize {:.2} ms, execute {:.2} ms)", r.rows, r.plan_ms, r.optimize_ms, r.exec_ms);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut data: Option<PathBuf> = None;
    let mut cmd: Option<String> = None;
    let mut explain = false;
    let mut opts = Options::default();
    let mut budget: Option<u64> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "-c" => cmd = args.next(),
            "--explain" => explain = true,
            "--no-opt" => opts.optimize = false,
            "--pair-budget" => budget = args.next().and_then(|v| v.parse().ok()),
            "-h" | "--help" => {
                println!("usage: quackless [--data DIR] [-c SQL] [--explain] [--no-opt] [--pair-budget N]");
                return;
            }
            other => {
                eprintln!("unknown argument {}", other);
                std::process::exit(2);
            }
        }
    }
    let mut db = Database::new();
    db.pair_budget = budget;
    if let Some(d) = &data {
        let t = Instant::now();
        if let Err(e) = load_tpch(&mut db, d) {
            eprintln!("load failed: {}", e);
            std::process::exit(1);
        }
        eprintln!("loaded {} tables in {:.1} s", db.tables().len(), t.elapsed().as_secs_f64());
    }
    let run = |db: &mut Database, sql: &str| -> bool {
        match db.execute(sql, opts) {
            Ok(r) => {
                print_result(&r, explain);
                true
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                false
            }
        }
    };
    if let Some(sql) = cmd {
        let ok = run(&mut db, &sql);
        std::process::exit(if ok { 0 } else { 1 });
    }
    let stdin = io::stdin();
    let tty = stdin.is_terminal();
    let mut buf = String::new();
    if tty {
        print!("quackless> ");
        let _ = io::stdout().flush();
    }
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        buf.push_str(&line);
        buf.push('\n');
        if line.trim_end().ends_with(';') {
            let sql = buf.trim().trim_end_matches(';').to_string();
            buf.clear();
            if !sql.is_empty() {
                run(&mut db, &sql);
            }
        }
        if tty {
            print!("{}", if buf.is_empty() { "quackless> " } else { "        ...> " });
            let _ = io::stdout().flush();
        }
    }
}
