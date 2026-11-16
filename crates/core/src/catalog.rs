//! In-memory tables and CSV loading.

use crate::ast::Query;
use crate::types::{parse_date, Column, ColumnBuilder, ColumnData, DataType, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub columns: Vec<(String, DataType)>,
    pub data: Vec<Rc<Column>>,
    pub rows: usize,
}

impl Table {
    pub fn empty(name: &str, columns: Vec<(String, DataType)>) -> Self {
        let data = columns.iter().map(|(_, t)| Rc::new(Column::nulls(*t, 0))).collect();
        Table { name: name.to_string(), columns, data, rows: 0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub tables: HashMap<String, Table>,
    pub views: HashMap<String, Query>,
    /// Exact distinct counts per (table, column), computed on first use by the
    /// join orderer and dropped whenever the table changes.
    pub ndv_cache: RefCell<HashMap<(String, usize), f64>>,
}

impl Catalog {
    pub fn get(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    /// Number of distinct non-NULL values in a column (cached).
    pub fn ndv(&self, table: &str, col: usize) -> Option<f64> {
        let key = (table.to_string(), col);
        if let Some(v) = self.ndv_cache.borrow().get(&key) {
            return Some(*v);
        }
        let t = self.tables.get(table)?;
        let c = t.data.get(col)?;
        let valid = |i: usize| c.validity.as_ref().map(|v| v[i]).unwrap_or(true);
        let n = match &c.data {
            ColumnData::Int(v) => v.iter().enumerate().filter(|(i, _)| valid(*i)).map(|(_, x)| *x).collect::<HashSet<i64>>().len(),
            ColumnData::Date(v) => v.iter().enumerate().filter(|(i, _)| valid(*i)).map(|(_, x)| *x).collect::<HashSet<i32>>().len(),
            ColumnData::Float(v) => v.iter().enumerate().filter(|(i, _)| valid(*i)).map(|(_, x)| x.to_bits()).collect::<HashSet<u64>>().len(),
            ColumnData::Bool(v) => v.iter().enumerate().filter(|(i, _)| valid(*i)).map(|(_, x)| *x).collect::<HashSet<bool>>().len(),
            ColumnData::Str(v) => (0..v.len()).filter(|i| valid(*i)).map(|i| v.get(i)).collect::<HashSet<&str>>().len(),
        };
        let n = (n as f64).max(1.0);
        self.ndv_cache.borrow_mut().insert(key, n);
        Some(n)
    }

    /// Forget cached statistics for a table after its rows change.
    pub fn invalidate(&self, table: &str) {
        self.ndv_cache.borrow_mut().retain(|k, _| k.0 != table);
    }

    /// Append rows (already cast to the table's column types).
    pub fn append_rows(&mut self, name: &str, rows: Vec<Vec<Value>>) -> Result<usize, String> {
        let t = self.tables.get_mut(name).ok_or_else(|| format!("Catalog error: table {} does not exist", name))?;
        let n = rows.len();
        let mut new_cols = Vec::with_capacity(t.columns.len());
        for (ci, (_, dt)) in t.columns.iter().enumerate() {
            let mut b = ColumnBuilder::new(*dt, t.rows + n);
            let old = &t.data[ci];
            for i in 0..old.len() {
                b.push_from(old, i);
            }
            for r in &rows {
                b.push(&r[ci]);
            }
            new_cols.push(Rc::new(b.finish()));
        }
        t.data = new_cols;
        t.rows += n;
        self.invalidate(name);
        Ok(n)
    }
}

/// Parse CSV text (with a header row) into columns of the given types.
/// Empty fields become NULL. This is the only place a library (`csv`) is used.
pub fn load_csv(name: &str, columns: &[(String, DataType)], text: &[u8]) -> Result<Table, String> {
    let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(text);
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    // map table columns to csv column positions by name
    let mut pos = Vec::with_capacity(columns.len());
    for (c, _) in columns {
        let p = headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(c))
            .ok_or_else(|| format!("CSV for {} has no column {}", name, c))?;
        pos.push(p);
    }
    let est = text.len() / 64 + 16;
    let mut builders: Vec<ColumnBuilder> = columns.iter().map(|(_, t)| ColumnBuilder::new(*t, est)).collect();
    let mut rec = csv::ByteRecord::new();
    let mut rows = 0usize;
    while rdr.read_byte_record(&mut rec).map_err(|e| e.to_string())? {
        for (ci, (_, dt)) in columns.iter().enumerate() {
            let raw = rec.get(pos[ci]).unwrap_or(b"");
            let b = &mut builders[ci];
            if raw.is_empty() && *dt != DataType::Str {
                b.push_null();
                continue;
            }
            let s = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
            let v = match dt {
                DataType::Int => Value::Int(fast_parse_i64(s).ok_or_else(|| format!("bad integer '{}' in {}", s, name))?),
                DataType::Float => Value::Float(s.parse::<f64>().map_err(|_| format!("bad number '{}' in {}", s, name))?),
                DataType::Date => Value::Date(parse_date(s).ok_or_else(|| format!("bad date '{}' in {}", s, name))?),
                DataType::Bool => Value::Bool(s == "true" || s == "1"),
                _ => Value::Str(s.to_string()),
            };
            b.push(&v);
        }
        rows += 1;
    }
    Ok(Table {
        name: name.to_string(),
        columns: columns.to_vec(),
        data: builders.into_iter().map(|b| Rc::new(b.finish())).collect(),
        rows,
    })
}

fn fast_parse_i64(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 18 {
        return s.parse().ok();
    }
    let (neg, digits) = if b[0] == b'-' { (true, &b[1..]) } else { (false, b) };
    if digits.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return s.parse().ok();
        }
        v = v * 10 + (c - b'0') as i64;
    }
    Some(if neg { -v } else { v })
}
