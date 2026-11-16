use crate::compare::Cell;
use quackless_core::ast::Statement;
use quackless_core::{Database, Options, QueryResult};
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().and_then(|p| p.parent()).unwrap().to_path_buf()
}

pub fn ours_cells(r: &QueryResult) -> Vec<Vec<Cell>> {
    (0..r.rows).map(|i| r.columns.iter().map(|c| Cell::from_value(&c.get(i))).collect()).collect()
}

pub fn duck_cells(rows: &[Vec<serde_json::Value>]) -> Vec<Vec<Cell>> {
    rows.iter().map(|r| r.iter().map(Cell::from_json).collect()).collect()
}

/// True when the statement's outermost query has an ORDER BY.
pub fn is_ordered(sql: &str) -> bool {
    match quackless_core::parser::parse_statement(sql) {
        Ok(Statement::Query(q)) => !q.order_by.is_empty(),
        _ => false,
    }
}

pub fn load_tpch(dir: &Path) -> Result<Database, String> {
    let mut db = Database::new();
    let ddl = std::fs::read_to_string(root().join("data/schema.sql")).map_err(|e| e.to_string())?;
    db.execute_script(&ddl, Options::default())?;
    for (name, _, _) in db.tables() {
        let p = dir.join(format!("{}.csv", name));
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        db.load_csv(&name, &bytes)?;
    }
    Ok(db)
}

pub fn strip(sql: &str) -> String {
    sql.trim().trim_end_matches(';').trim().to_string()
}

pub fn json_str(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}
