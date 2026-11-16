//! Comparing a Quackless result with a DuckDB result.
//!
//! Rules (also in the README):
//! - NULL equals only NULL.
//! - Integers on both sides compare exactly. Strings and dates (ISO text) compare exactly.
//! - Anything else numeric compares with a relative tolerance of 1e-9 (absolute 1e-9
//!   near zero). Quackless stores DECIMAL as a 64-bit float, DuckDB as an exact
//!   decimal, so sums over millions of rows differ in the last bits.
//! - Without a top-level ORDER BY, rows are compared as a multiset.

use quackless_core::types::Value;
use serde_json::Value as J;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

pub const REL_TOL: f64 = 1e-9;

impl Cell {
    pub fn from_value(v: &Value) -> Cell {
        match v {
            Value::Null => Cell::Null,
            Value::Bool(b) => Cell::Bool(*b),
            Value::Int(i) => Cell::Int(*i),
            Value::Float(f) => Cell::Float(*f),
            other => Cell::Str(other.to_string()),
        }
    }

    pub fn from_json(v: &J) -> Cell {
        match v {
            J::Null => Cell::Null,
            J::Bool(b) => Cell::Bool(*b),
            J::Number(n) => match n.as_i64() {
                Some(i) => Cell::Int(i),
                None => Cell::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            J::String(s) if s == "inf" || s == "-inf" || s == "nan" => Cell::Float(s.parse().unwrap_or(f64::NAN)),
            J::String(s) => match s.parse::<i64>() {
                // integers beyond 2^53 arrive as text
                Ok(i) if s.len() > 15 => Cell::Int(i),
                _ => Cell::Str(s.clone()),
            },
            other => Cell::Str(other.to_string()),
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Cell::Int(i) => Some(*i as f64),
            Cell::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Key used to sort rows before a multiset comparison: floats rounded to
    /// 9 significant digits so tolerance-equal values usually sort together.
    fn sort_key(&self) -> (u8, f64, String) {
        match self {
            Cell::Null => (0, 0.0, String::new()),
            Cell::Bool(b) => (1, *b as u8 as f64, String::new()),
            Cell::Int(_) | Cell::Float(_) => {
                let x = self.as_f64().unwrap();
                let r = if x == 0.0 || !x.is_finite() {
                    x
                } else {
                    let mag = 10f64.powi(8 - x.abs().log10().floor() as i32);
                    (x * mag).round() / mag
                };
                (2, r, String::new())
            }
            Cell::Str(s) => (3, 0.0, s.clone()),
        }
    }
}

pub fn cells_equal(a: &Cell, b: &Cell) -> bool {
    match (a, b) {
        (Cell::Null, Cell::Null) => true,
        (Cell::Null, _) | (_, Cell::Null) => false,
        (Cell::Int(x), Cell::Int(y)) => x == y,
        (Cell::Bool(x), Cell::Bool(y)) => x == y,
        (Cell::Str(x), Cell::Str(y)) => x == y,
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => {
                if x == y {
                    return true;
                }
                if x.is_infinite() || y.is_infinite() {
                    return false;
                }
                if x.is_nan() || y.is_nan() {
                    return x.is_nan() && y.is_nan();
                }
                let d = (x - y).abs();
                d <= 1e-9 || d <= REL_TOL * x.abs().max(y.abs())
            }
            _ => false,
        },
    }
}

fn cmp_rows(a: &[Cell], b: &[Cell]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let (kx, ky) = (x.sort_key(), y.sort_key());
        let o = kx.0.cmp(&ky.0).then(kx.1.total_cmp(&ky.1)).then_with(|| kx.2.cmp(&ky.2));
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

fn show(r: &[Cell]) -> String {
    r.iter()
        .map(|c| match c {
            Cell::Null => "NULL".to_string(),
            Cell::Bool(b) => b.to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => format!("{}", f),
            Cell::Str(s) => s.clone(),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Ok(()) when equal, otherwise a short description of the first difference.
pub fn compare(ours: &[Vec<Cell>], theirs: &[Vec<Cell>], ordered: bool) -> Result<(), String> {
    if ours.len() != theirs.len() {
        return Err(format!("row count {} vs DuckDB {}", ours.len(), theirs.len()));
    }
    if let (Some(a), Some(b)) = (ours.first(), theirs.first()) {
        if a.len() != b.len() {
            return Err(format!("column count {} vs DuckDB {}", a.len(), b.len()));
        }
    }
    let check = |a: &[Vec<Cell>], b: &[Vec<Cell>]| -> Result<(), String> {
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            if !x.iter().zip(y).all(|(p, q)| cells_equal(p, q)) {
                return Err(format!("row {}: [{}] vs DuckDB [{}]", i + 1, show(x), show(y)));
            }
        }
        Ok(())
    };
    if ordered {
        if let Err(e) = check(ours, theirs) {
            // same rows in a different order still fails, but say so
            let mut a = ours.to_vec();
            let mut b = theirs.to_vec();
            a.sort_by(|x, y| cmp_rows(x, y));
            b.sort_by(|x, y| cmp_rows(x, y));
            if check(&a, &b).is_ok() {
                return Err(format!("same rows, different order ({})", e));
            }
            return Err(e);
        }
        Ok(())
    } else {
        let mut a = ours.to_vec();
        let mut b = theirs.to_vec();
        a.sort_by(|x, y| cmp_rows(x, y));
        b.sort_by(|x, y| cmp_rows(x, y));
        check(&a, &b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_exact_floats_tolerant() {
        assert!(cells_equal(&Cell::Int(3), &Cell::Int(3)));
        assert!(!cells_equal(&Cell::Int(3), &Cell::Int(4)));
        assert!(cells_equal(&Cell::Float(1193053.2252999984), &Cell::Float(1193053.2253)));
        assert!(!cells_equal(&Cell::Float(1193053.22), &Cell::Float(1193053.2253)));
        assert!(cells_equal(&Cell::Float(380456.0), &Cell::Int(380456)));
        assert!(cells_equal(&Cell::Float(f64::INFINITY), &Cell::Float(f64::INFINITY)));
        assert!(!cells_equal(&Cell::Float(f64::INFINITY), &Cell::Float(f64::NEG_INFINITY)));
        assert!(!cells_equal(&Cell::Null, &Cell::Int(0)));
        assert!(cells_equal(&Cell::Null, &Cell::Null));
        assert!(!cells_equal(&Cell::Str("a".into()), &Cell::Str("a ".into())));
    }

    #[test]
    fn multiset_ignores_order_unless_ordered() {
        let a = vec![vec![Cell::Int(1)], vec![Cell::Int(2)]];
        let b = vec![vec![Cell::Int(2)], vec![Cell::Int(1)]];
        assert!(compare(&a, &b, false).is_ok());
        let e = compare(&a, &b, true).unwrap_err();
        assert!(e.contains("different order"));
        assert!(compare(&a, &a[..1], false).unwrap_err().contains("row count"));
    }

    #[test]
    fn json_cells() {
        assert_eq!(Cell::from_json(&serde_json::json!(5)), Cell::Int(5));
        assert_eq!(Cell::from_json(&serde_json::json!(0.5)), Cell::Float(0.5));
        assert_eq!(Cell::from_json(&serde_json::json!("1995-03-15")), Cell::Str("1995-03-15".into()));
        assert_eq!(Cell::from_json(&serde_json::json!("9007199254740993")), Cell::Int(9007199254740993));
    }
}
