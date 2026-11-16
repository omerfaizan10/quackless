//! Talks to scripts/duck_oracle.py (a long-running DuckDB process).

use serde_json::{json, Value as J};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct Oracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct DuckResult {
    #[allow(dead_code)] // kept for debugging output
    pub columns: Vec<String>,
    pub rows: Vec<Vec<J>>,
}

impl Oracle {
    pub fn spawn(root: &Path) -> Result<Self, String> {
        let py = root.join(".venv/bin/python");
        let mut child = Command::new(&py)
            .arg("-I")
            .arg(root.join("scripts/duck_oracle.py"))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start {}: {}", py.display(), e))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Ok(Oracle { child, stdin, stdout })
    }

    fn call(&mut self, req: J) -> Result<J, String> {
        writeln!(self.stdin, "{}", req).map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Err("oracle exited".into());
        }
        let v: J = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if v["ok"].as_bool() == Some(true) {
            Ok(v)
        } else {
            Err(v["error"].as_str().unwrap_or("unknown error").to_string())
        }
    }

    pub fn load_tpch(&mut self, dir: &str) -> Result<(), String> {
        self.call(json!({"op": "load_tpch", "dir": dir})).map(|_| ())
    }

    pub fn reset(&mut self) -> Result<(), String> {
        self.call(json!({"op": "reset"})).map(|_| ())
    }

    pub fn query(&mut self, sql: &str) -> Result<DuckResult, String> {
        let v = self.call(json!({"op": "query", "sql": sql}))?;
        let columns = v["columns"].as_array().map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
        let rows = v["rows"].as_array().map(|a| a.iter().map(|r| r.as_array().cloned().unwrap_or_default()).collect()).unwrap_or_default();
        Ok(DuckResult { columns, rows })
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
