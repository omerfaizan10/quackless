//! sqllogictest runner (SQLite's format), emulating the "sqlite" engine label.
//!
//! Formatting follows SQLite's reference harness: an `I` column prints the
//! value as an integer (reals truncate), `R` prints "%.3f", `T` prints text,
//! NULL prints "NULL", an empty string "(empty)". Integer division truncates.
//! Results above the hash threshold are compared by the MD5 of the values.

use md5::{Digest, Md5};
use quackless_core::types::Value;
use quackless_core::{Database, Options};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Kind {
    StatementOk,
    StatementError,
    Query { types: String, sort: String, expected: Vec<String>, hash: Option<(usize, String)> },
}

#[derive(Debug, Clone)]
pub struct Record {
    pub line: usize,
    pub sql: String,
    pub kind: Kind,
}

/// Parse a file into the records the sqlite engine would run, plus the hash threshold.
pub fn parse(text: &str) -> Vec<Record> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut skip = false;
    while i < lines.len() {
        let l = lines[i].trim_end();
        let words: Vec<&str> = l.split_whitespace().collect();
        if words.is_empty() || l.starts_with('#') {
            i += 1;
            continue;
        }
        match words[0] {
            "halt" => {
                if !skip {
                    break;
                }
                skip = false;
                i += 1;
            }
            "hash-threshold" => i += 1,
            "skipif" => {
                if words.get(1) == Some(&"sqlite") {
                    skip = true;
                }
                i += 1;
            }
            "onlyif" => {
                if words.get(1) != Some(&"sqlite") {
                    skip = true;
                }
                i += 1;
            }
            "statement" | "query" => {
                let start = i + 1;
                let head = words.clone();
                i += 1;
                let mut sql = Vec::new();
                while i < lines.len() && !lines[i].trim().is_empty() && lines[i].trim() != "----" {
                    sql.push(lines[i]);
                    i += 1;
                }
                let mut expected = Vec::new();
                if i < lines.len() && lines[i].trim() == "----" {
                    i += 1;
                    while i < lines.len() && !lines[i].trim().is_empty() {
                        expected.push(lines[i].trim_end().to_string());
                        i += 1;
                    }
                }
                let kind = if head[0] == "statement" {
                    if head.get(1) == Some(&"error") { Kind::StatementError } else { Kind::StatementOk }
                } else {
                    let types = head.get(1).unwrap_or(&"").to_string();
                    let sort = head.get(2).unwrap_or(&"nosort").to_string();
                    let mut hash = None;
                    if expected.len() == 1 {
                        let w: Vec<&str> = expected[0].split_whitespace().collect();
                        if w.len() == 5 && w[1] == "values" && w[2] == "hashing" {
                            hash = Some((w[0].parse().unwrap_or(0), w[4].to_string()));
                        }
                    }
                    Kind::Query { types, sort, expected, hash }
                };
                if !skip {
                    out.push(Record { line: start, sql: sql.join("\n"), kind });
                }
                skip = false;
            }
            _ => i += 1,
        }
    }
    out
}

fn clean(s: String) -> String {
    if s.is_empty() {
        return "(empty)".into();
    }
    s.chars().map(|c| if (c as u32) < 0x20 || (c as u32) > 0x7e { '@' } else { c }).collect()
}

/// Leading numeric prefix of a string, like SQLite's text-to-number conversion.
fn text_num(s: &str) -> f64 {
    let t = s.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    let mut seen_dot = false;
    let mut seen_e = false;
    while end < b.len() {
        let c = b[end];
        let ok = c.is_ascii_digit()
            || ((c == b'-' || c == b'+') && (end == 0 || matches!(b[end - 1], b'e' | b'E')))
            || (c == b'.' && !seen_dot && !seen_e)
            || ((c == b'e' || c == b'E') && !seen_e && end > 0);
        if !ok {
            break;
        }
        seen_dot |= c == b'.';
        seen_e |= c == b'e' || c == b'E';
        end += 1;
    }
    let mut p = &t[..end];
    while !p.is_empty() && p.parse::<f64>().is_err() {
        p = &p[..p.len() - 1];
    }
    p.parse().unwrap_or(0.0)
}

pub fn format_value(v: &Value, ty: char) -> String {
    if matches!(v, Value::Null) {
        return "NULL".into();
    }
    match ty {
        'I' => match v {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => (*f as i64).to_string(),
            Value::Bool(b) => (*b as i64).to_string(),
            Value::Str(s) => (text_num(s) as i64).to_string(),
            other => clean(other.to_string()),
        },
        'R' => match v {
            Value::Int(i) => format!("{:.3}", *i as f64),
            Value::Float(f) => format!("{:.3}", f),
            Value::Bool(b) => format!("{:.3}", *b as i64 as f64),
            Value::Str(s) => format!("{:.3}", text_num(s)),
            other => clean(other.to_string()),
        },
        _ => match v {
            Value::Bool(b) => (*b as i64).to_string(),
            Value::Float(f) => {
                if f.fract() == 0.0 && f.abs() < 1e15 { format!("{:.1}", f) } else { format!("{}", f) }
            }
            other => clean(other.to_string()),
        },
    }
}

/// Run one record. Ok(()) on pass, Err(reason) otherwise.
pub fn run_record(db: &mut Database, r: &Record) -> Result<(), String> {
    let opts = Options { optimize: true, int_div: true, record: false };
    match &r.kind {
        Kind::StatementOk => db.execute_script(&r.sql, opts).map(|_| ()),
        Kind::StatementError => match db.execute_script(&r.sql, opts) {
            Ok(_) => Err("expected an error".into()),
            Err(_) => Ok(()),
        },
        Kind::Query { types, sort, expected, hash } => {
            let res = db.execute(&r.sql, opts)?;
            let ty: Vec<char> = types.chars().collect();
            if res.columns.len() != ty.len() {
                return Err(format!("{} columns, expected {}", res.columns.len(), ty.len()));
            }
            let mut rows: Vec<Vec<String>> = (0..res.rows)
                .map(|i| res.columns.iter().enumerate().map(|(c, col)| format_value(&col.get(i), ty[c])).collect())
                .collect();
            let mut vals: Vec<String> = match sort.as_str() {
                "rowsort" => {
                    rows.sort();
                    rows.into_iter().flatten().collect()
                }
                "valuesort" => {
                    let mut v: Vec<String> = rows.into_iter().flatten().collect();
                    v.sort();
                    v
                }
                _ => rows.into_iter().flatten().collect(),
            };
            match hash {
                Some((n, h)) => {
                    let mut m = Md5::new();
                    for v in &vals {
                        m.update(v.as_bytes());
                        m.update(b"\n");
                    }
                    let got = format!("{:x}", m.finalize());
                    if vals.len() != *n || &got != h {
                        return Err(format!("{} values hashing to {}, expected {} hashing to {}", vals.len(), got, n, h));
                    }
                    Ok(())
                }
                None => {
                    // the reference format is one value per line; some files put a row per line
                    let mut exp: Vec<String> = if expected.len() == vals.len() {
                        expected.clone()
                    } else {
                        expected.iter().flat_map(|l| l.split_whitespace().map(|s| s.to_string())).collect()
                    };
                    if exp != vals {
                        vals.truncate(12);
                        exp.truncate(12);
                        return Err(format!("got [{}] expected [{}]", vals.join(" "), exp.join(" ")));
                    }
                    Ok(())
                }
            }
        }
    }
}

/// Child mode: run every statement, and query records with index > `after`,
/// printing "<index>\t<ok|fail>\t<reason>" per record.
pub fn child(file: &Path, after: i64) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let recs = parse(&text);
    let mut db = Database::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for (i, r) in recs.iter().enumerate() {
        let is_query = matches!(r.kind, Kind::Query { .. });
        if (i as i64) <= after && is_query {
            continue;
        }
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_record(&mut db, r)));
        let res = match res {
            Ok(x) => x,
            Err(_) => Err("panic".into()),
        };
        if (i as i64) > after {
            let reason = res.err().unwrap_or_default().replace(['\n', '\t'], " ");
            let _ = writeln!(out, "{}\t{}\t{}", i, if reason.is_empty() { "ok" } else { "fail" }, reason.chars().take(240).collect::<String>());
            let _ = out.flush();
        }
    }
    Ok(())
}

pub struct FileOutcome {
    pub total: usize,
    pub passed: usize,
    pub statements: usize,
    pub statements_passed: usize,
    pub timeouts: usize,
    pub failures: Vec<(usize, String, String)>,
}

/// Parent mode: run a file in child processes, restarting after a hung record.
pub fn run_file(file: &Path, timeout: Duration) -> Result<FileOutcome, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let recs = parse(&text);
    let mut status: Vec<Option<(bool, String)>> = vec![None; recs.len()];
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut after: i64 = -1;
    let mut timeouts = 0;
    loop {
        let mut child = Command::new(&exe)
            .arg("slt-child")
            .arg(file)
            .arg(after.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let mut last = after;
        let mut hung = false;
        loop {
            match rx.recv_timeout(timeout) {
                Ok(l) => {
                    let mut p = l.splitn(3, '\t');
                    let i: usize = p.next().unwrap_or("0").parse().unwrap_or(0);
                    let ok = p.next() == Some("ok");
                    let reason = p.next().unwrap_or("").to_string();
                    if i < status.len() {
                        status[i] = Some((ok, reason));
                    }
                    last = i as i64;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    hung = true;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        if !hung {
            // a crash (not a hang) also leaves the rest unreported: mark next as failed and resume
            let next = (last + 1) as usize;
            if next < recs.len() && status[next].is_none() {
                status[next] = Some((false, "engine crashed".into()));
                after = next as i64;
                continue;
            }
            break;
        }
        // the record after `last` hung
        let next = (last + 1) as usize;
        if next >= recs.len() {
            break;
        }
        timeouts += 1;
        status[next] = Some((false, format!("timeout after {} s", timeout.as_secs())));
        if !matches!(recs[next].kind, Kind::Query { .. }) {
            // a hung statement would leave the database incomplete; stop the file here
            for s in status.iter_mut().skip(next + 1) {
                if s.is_none() {
                    *s = Some((false, "not run (statement timed out)".into()));
                }
            }
            break;
        }
        after = next as i64;
    }
    let mut o = FileOutcome { total: recs.len(), passed: 0, statements: 0, statements_passed: 0, timeouts, failures: Vec::new() };
    for (i, r) in recs.iter().enumerate() {
        let (ok, reason) = status[i].clone().unwrap_or((false, "not run".into()));
        let is_stmt = !matches!(r.kind, Kind::Query { .. });
        if is_stmt {
            o.statements += 1;
        }
        if ok {
            o.passed += 1;
            if is_stmt {
                o.statements_passed += 1;
            }
        } else {
            o.failures.push((r.line, r.sql.lines().next().unwrap_or("").chars().take(160).collect(), reason));
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_and_conditions() {
        let t = "statement ok\nCREATE TABLE t1(a INTEGER)\n\nskipif sqlite\nquery I nosort\nSELECT 1\n----\n1\n\nonlyif mysql\nstatement ok\nX\n\nquery II rowsort\nSELECT a, a FROM t1\n----\n30 values hashing to abc\n";
        let r = parse(t);
        assert_eq!(r.len(), 2);
        assert!(matches!(r[0].kind, Kind::StatementOk));
        match &r[1].kind {
            Kind::Query { types, sort, hash, .. } => {
                assert_eq!(types, "II");
                assert_eq!(sort, "rowsort");
                assert_eq!(hash, &Some((30, "abc".to_string())));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn formats_like_sqlite() {
        assert_eq!(format_value(&Value::Float(2.9), 'I'), "2");
        assert_eq!(format_value(&Value::Int(7), 'R'), "7.000");
        assert_eq!(format_value(&Value::Str(String::new()), 'T'), "(empty)");
        assert_eq!(format_value(&Value::Null, 'I'), "NULL");
        assert_eq!(format_value(&Value::Str("12abc".into()), 'I'), "12");
    }

    #[test]
    fn runs_a_tiny_script() {
        let t = "statement ok\nCREATE TABLE t1(a INTEGER, b INTEGER)\n\nstatement ok\nINSERT INTO t1 VALUES(1,2),(3,4)\n\nquery I rowsort\nSELECT a+b FROM t1\n----\n3\n7\n\nquery I nosort\nSELECT 7/2\n----\n3\n";
        let mut db = Database::new();
        for r in parse(t) {
            run_record(&mut db, &r).unwrap();
        }
    }
}
