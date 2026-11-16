//! Column vectors, scalar values and batches.
//!
//! I use my own vectors instead of arrow-rs: the engine only needs five
//! physical types, and keeping the representation small keeps the WASM
//! bundle small (arrow-rs alone adds several hundred KB of wasm).

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    Null,
    Bool,
    Int,
    Float,
    Str,
    Date,
    Interval,
}

impl DataType {
    pub fn is_numeric(self) -> bool {
        matches!(self, DataType::Int | DataType::Float)
    }
    pub fn name(self) -> &'static str {
        match self {
            DataType::Null => "NULL",
            DataType::Bool => "BOOLEAN",
            DataType::Int => "BIGINT",
            DataType::Float => "DOUBLE",
            DataType::Str => "VARCHAR",
            DataType::Date => "DATE",
            DataType::Interval => "INTERVAL",
        }
    }
}

/// A single scalar value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Days since 1970-01-01.
    Date(i32),
    Interval { months: i32, days: i32 },
}

impl Value {
    pub fn data_type(&self) -> DataType {
        match self {
            Value::Null => DataType::Null,
            Value::Bool(_) => DataType::Bool,
            Value::Int(_) => DataType::Int,
            Value::Float(_) => DataType::Float,
            Value::Str(_) => DataType::Str,
            Value::Date(_) => DataType::Date,
            Value::Interval { .. } => DataType::Interval,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    /// Total order used by ORDER BY, MIN/MAX and DISTINCT. NULLs sort last.
    pub fn total_cmp(&self, other: &Value) -> Ordering {
        use Value::*;
        match (self, other) {
            (Null, Null) => Ordering::Equal,
            (Null, _) => Ordering::Greater,
            (_, Null) => Ordering::Less,
            (Int(a), Int(b)) => a.cmp(b),
            (Int(a), Float(b)) => crate::exec::cmp_f64(*a as f64, *b),
            (Float(a), Int(b)) => crate::exec::cmp_f64(*a, *b as f64),
            (Float(a), Float(b)) => crate::exec::cmp_f64(*a, *b),
            (Str(a), Str(b)) => a.cmp(b),
            (Date(a), Date(b)) => a.cmp(b),
            (Bool(a), Bool(b)) => a.cmp(b),
            _ => Ordering::Equal,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(x) => write!(f, "{}", format_float(*x)),
            Value::Str(s) => write!(f, "{}", s),
            Value::Date(d) => write!(f, "{}", format_date(*d)),
            Value::Interval { months, days } => {
                if *months != 0 && *days != 0 {
                    write!(f, "{} months {} days", months, days)
                } else if *months != 0 {
                    write!(f, "{} months", months)
                } else {
                    write!(f, "{} days", days)
                }
            }
        }
    }
}

pub fn format_float(x: f64) -> String {
    if x.is_finite() && x == x.trunc() && x.abs() < 1e15 {
        format!("{:.1}", x)
    } else {
        format!("{}", x)
    }
}

// ---------------------------------------------------------------------------
// Dates (proleptic Gregorian, days since the Unix epoch)
// ---------------------------------------------------------------------------

/// Days from civil, after Howard Hinnant's algorithm.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i32
}

pub fn civil_from_days(z: i32) -> (i32, u32, u32) {
    let z = z as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

pub fn parse_date(s: &str) -> Option<i32> {
    let s = s.trim();
    let mut it = s.splitn(3, '-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let rest = it.next()?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() != rest.len() && !rest[digits.len()..].starts_with(' ') {
        return None;
    }
    let d: u32 = digits.parse().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

pub fn format_date(d: i32) -> String {
    let (y, m, dd) = civil_from_days(d);
    format!("{:04}-{:02}-{:02}", y, m, dd)
}

/// Date plus interval, clamping the day to the end of the target month
/// (the same rule DuckDB and PostgreSQL use).
pub fn add_interval(d: i32, months: i32, days: i32) -> i32 {
    let mut out = d;
    if months != 0 {
        let (y, m, dd) = civil_from_days(d);
        let total = y * 12 + (m as i32 - 1) + months;
        let ny = total.div_euclid(12);
        let nm = (total.rem_euclid(12) + 1) as u32;
        let nd = dd.min(days_in_month(ny, nm));
        out = days_from_civil(ny, nm, nd);
    }
    out + days
}

// ---------------------------------------------------------------------------
// String vector: one contiguous byte buffer plus offsets.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct StrVec {
    pub offsets: Vec<u32>,
    pub bytes: Vec<u8>,
}

impl StrVec {
    pub fn new() -> Self {
        StrVec { offsets: vec![0], bytes: Vec::new() }
    }
    pub fn with_capacity(n: usize, bytes: usize) -> Self {
        let mut offsets = Vec::with_capacity(n + 1);
        offsets.push(0);
        StrVec { offsets, bytes: Vec::with_capacity(bytes) }
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    #[inline]
    pub fn get(&self, i: usize) -> &str {
        let a = self.offsets[i] as usize;
        let b = self.offsets[i + 1] as usize;
        // Safety: only valid UTF-8 is ever pushed.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[a..b]) }
    }
    #[inline]
    pub fn push(&mut self, s: &str) {
        self.bytes.extend_from_slice(s.as_bytes());
        self.offsets.push(self.bytes.len() as u32);
    }
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        (0..self.len()).map(move |i| self.get(i))
    }
}

// ---------------------------------------------------------------------------
// Column = typed vector + optional validity mask (None means no NULLs).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ColumnData {
    Bool(Vec<bool>),
    Int(Vec<i64>),
    Float(Vec<f64>),
    Str(StrVec),
    Date(Vec<i32>),
}

#[derive(Debug, Clone)]
pub struct Column {
    pub data: ColumnData,
    pub validity: Option<Vec<bool>>,
}

impl Column {
    pub fn new(data: ColumnData) -> Self {
        Column { data, validity: None }
    }
    pub fn with_validity(data: ColumnData, validity: Option<Vec<bool>>) -> Self {
        let validity = match validity {
            Some(v) if v.iter().all(|b| *b) => None,
            other => other,
        };
        Column { data, validity }
    }
    pub fn len(&self) -> usize {
        match &self.data {
            ColumnData::Bool(v) => v.len(),
            ColumnData::Int(v) => v.len(),
            ColumnData::Float(v) => v.len(),
            ColumnData::Str(v) => v.len(),
            ColumnData::Date(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn data_type(&self) -> DataType {
        match &self.data {
            ColumnData::Bool(_) => DataType::Bool,
            ColumnData::Int(_) => DataType::Int,
            ColumnData::Float(_) => DataType::Float,
            ColumnData::Str(_) => DataType::Str,
            ColumnData::Date(_) => DataType::Date,
        }
    }
    #[inline]
    pub fn is_valid(&self, i: usize) -> bool {
        match &self.validity {
            None => true,
            Some(v) => v[i],
        }
    }
    pub fn null_count(&self) -> usize {
        self.validity.as_ref().map(|v| v.iter().filter(|b| !**b).count()).unwrap_or(0)
    }
    pub fn get(&self, i: usize) -> Value {
        if !self.is_valid(i) {
            return Value::Null;
        }
        match &self.data {
            ColumnData::Bool(v) => Value::Bool(v[i]),
            ColumnData::Int(v) => Value::Int(v[i]),
            ColumnData::Float(v) => Value::Float(v[i]),
            ColumnData::Str(v) => Value::Str(v.get(i).to_string()),
            ColumnData::Date(v) => Value::Date(v[i]),
        }
    }

    /// An all-NULL column of the given type.
    pub fn nulls(dt: DataType, n: usize) -> Column {
        let mut b = ColumnBuilder::new(dt, n);
        for _ in 0..n {
            b.push_null();
        }
        b.finish()
    }

    /// A column repeating one value n times.
    pub fn repeat(v: &Value, dt: DataType, n: usize) -> Column {
        let mut b = ColumnBuilder::new(if dt == DataType::Null { DataType::Int } else { dt }, n);
        for _ in 0..n {
            b.push(v);
        }
        b.finish()
    }

    /// Gather rows by index (the core of filter, join and sort output).
    pub fn take(&self, idx: &[u32]) -> Column {
        let data = match &self.data {
            ColumnData::Bool(v) => ColumnData::Bool(idx.iter().map(|&i| v[i as usize]).collect()),
            ColumnData::Int(v) => ColumnData::Int(idx.iter().map(|&i| v[i as usize]).collect()),
            ColumnData::Float(v) => ColumnData::Float(idx.iter().map(|&i| v[i as usize]).collect()),
            ColumnData::Date(v) => ColumnData::Date(idx.iter().map(|&i| v[i as usize]).collect()),
            ColumnData::Str(v) => {
                let mut out = StrVec::with_capacity(idx.len(), idx.len() * 8);
                for &i in idx {
                    out.push(v.get(i as usize));
                }
                ColumnData::Str(out)
            }
        };
        let validity = self.validity.as_ref().map(|v| idx.iter().map(|&i| v[i as usize]).collect());
        Column::with_validity(data, validity)
    }

    /// Gather with optional indices: None produces NULL (used by LEFT JOIN).
    pub fn take_opt(&self, idx: &[Option<u32>]) -> Column {
        let mut b = ColumnBuilder::new(self.data_type(), idx.len());
        for i in idx {
            match i {
                Some(i) => b.push_from(self, *i as usize),
                None => b.push_null(),
            }
        }
        b.finish()
    }

    pub fn slice(&self, start: usize, len: usize) -> Column {
        let idx: Vec<u32> = (start as u32..(start + len) as u32).collect();
        self.take(&idx)
    }

    pub fn concat(cols: &[Column]) -> Column {
        let dt = cols.iter().map(|c| c.data_type()).next().unwrap_or(DataType::Int);
        let n: usize = cols.iter().map(|c| c.len()).sum();
        let mut b = ColumnBuilder::new(dt, n);
        for c in cols {
            for i in 0..c.len() {
                b.push_from(c, i);
            }
        }
        b.finish()
    }

    pub fn cast(&self, to: DataType) -> Result<Column, String> {
        if self.data_type() == to {
            return Ok(self.clone());
        }
        let n = self.len();
        let mut b = ColumnBuilder::new(to, n);
        for i in 0..n {
            let v = cast_value(&self.get(i), to)?;
            b.push(&v);
        }
        Ok(b.finish())
    }
}

pub fn cast_value(v: &Value, to: DataType) -> Result<Value, String> {
    use Value::*;
    Ok(match (v, to) {
        (Null, _) => Null,
        (v, t) if v.data_type() == t => v.clone(),
        (Int(i), DataType::Float) => Float(*i as f64),
        (Float(f), DataType::Int) => Int(f.round() as i64),
        (Bool(b), DataType::Int) => Int(*b as i64),
        (Int(i), DataType::Bool) => Bool(*i != 0),
        (Str(s), DataType::Int) => Int(s.trim().parse::<i64>().map_err(|_| format!("could not convert string '{}' to BIGINT", s))?),
        (Str(s), DataType::Float) => Float(s.trim().parse::<f64>().map_err(|_| format!("could not convert string '{}' to DOUBLE", s))?),
        (Str(s), DataType::Date) => Date(parse_date(s).ok_or_else(|| format!("invalid date '{}'", s))?),
        (Str(s), DataType::Bool) => match s.to_ascii_lowercase().as_str() {
            "true" | "t" | "1" => Bool(true),
            "false" | "f" | "0" => Bool(false),
            _ => return Err(format!("could not convert string '{}' to BOOLEAN", s)),
        },
        (v, DataType::Str) => Str(v.to_string()),
        (v, t) => return Err(format!("cannot cast {} to {}", v.data_type().name(), t.name())),
    })
}

/// Appends values into a typed vector.
pub struct ColumnBuilder {
    data: ColumnData,
    validity: Vec<bool>,
    has_null: bool,
}

impl ColumnBuilder {
    pub fn new(dt: DataType, cap: usize) -> Self {
        let data = match dt {
            DataType::Bool => ColumnData::Bool(Vec::with_capacity(cap)),
            DataType::Int | DataType::Null => ColumnData::Int(Vec::with_capacity(cap)),
            DataType::Float => ColumnData::Float(Vec::with_capacity(cap)),
            DataType::Str | DataType::Interval => ColumnData::Str(StrVec::with_capacity(cap, cap * 8)),
            DataType::Date => ColumnData::Date(Vec::with_capacity(cap)),
        };
        ColumnBuilder { data, validity: Vec::with_capacity(cap), has_null: false }
    }
    pub fn push_null(&mut self) {
        self.has_null = true;
        self.validity.push(false);
        match &mut self.data {
            ColumnData::Bool(v) => v.push(false),
            ColumnData::Int(v) => v.push(0),
            ColumnData::Float(v) => v.push(0.0),
            ColumnData::Str(v) => v.push(""),
            ColumnData::Date(v) => v.push(0),
        }
    }
    /// Push a value, converting numerics if the builder type differs.
    pub fn push(&mut self, val: &Value) {
        match (&mut self.data, val) {
            (_, Value::Null) => return self.push_null(),
            (ColumnData::Bool(v), Value::Bool(b)) => v.push(*b),
            (ColumnData::Int(v), Value::Int(i)) => v.push(*i),
            (ColumnData::Int(v), Value::Float(f)) => v.push(*f as i64),
            (ColumnData::Float(v), Value::Float(f)) => v.push(*f),
            (ColumnData::Float(v), Value::Int(i)) => v.push(*i as f64),
            (ColumnData::Str(v), Value::Str(s)) => v.push(s),
            (ColumnData::Str(v), other) => v.push(&other.to_string()),
            (ColumnData::Date(v), Value::Date(d)) => v.push(*d),
            _ => return self.push_null(),
        }
        self.validity.push(true);
    }
    #[inline]
    pub fn push_from(&mut self, c: &Column, i: usize) {
        if !c.is_valid(i) {
            return self.push_null();
        }
        match (&mut self.data, &c.data) {
            (ColumnData::Bool(a), ColumnData::Bool(b)) => a.push(b[i]),
            (ColumnData::Int(a), ColumnData::Int(b)) => a.push(b[i]),
            (ColumnData::Float(a), ColumnData::Float(b)) => a.push(b[i]),
            (ColumnData::Float(a), ColumnData::Int(b)) => a.push(b[i] as f64),
            (ColumnData::Str(a), ColumnData::Str(b)) => a.push(b.get(i)),
            (ColumnData::Date(a), ColumnData::Date(b)) => a.push(b[i]),
            _ => return self.push(&c.get(i)),
        }
        self.validity.push(true);
    }
    pub fn finish(self) -> Column {
        let validity = if self.has_null { Some(self.validity) } else { None };
        Column { data: self.data, validity }
    }
}

/// A batch of rows stored column-wise. All columns have the same length.
#[derive(Debug, Clone, Default)]
pub struct Batch {
    pub columns: Vec<Column>,
    pub rows: usize,
}

impl Batch {
    pub fn new(columns: Vec<Column>, rows: usize) -> Self {
        Batch { columns, rows }
    }
    pub fn take(&self, idx: &[u32]) -> Batch {
        Batch { columns: self.columns.iter().map(|c| c.take(idx)).collect(), rows: idx.len() }
    }
    pub fn concat(batches: &[Batch], types: &[DataType]) -> Batch {
        let rows = batches.iter().map(|b| b.rows).sum();
        let columns = (0..types.len())
            .map(|ci| {
                let parts: Vec<Column> = batches.iter().map(|b| b.columns[ci].clone()).collect();
                if parts.is_empty() {
                    Column::nulls(types[ci], 0)
                } else if parts.len() == 1 {
                    parts.into_iter().next().unwrap()
                } else {
                    Column::concat(&parts)
                }
            })
            .collect();
        Batch { columns, rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_roundtrip() {
        for s in ["1970-01-01", "1992-02-29", "1998-12-01", "2000-03-01", "1969-12-31"] {
            let d = parse_date(s).unwrap();
            assert_eq!(format_date(d), s);
        }
        assert_eq!(parse_date("1970-01-02"), Some(1));
        assert_eq!(parse_date("1993-02-30"), None);
    }

    #[test]
    fn interval_month_clamps() {
        let d = parse_date("1995-01-31").unwrap();
        assert_eq!(format_date(add_interval(d, 1, 0)), "1995-02-28");
        let d = parse_date("1998-12-01").unwrap();
        assert_eq!(format_date(add_interval(d, 0, -90)), "1998-09-02");
        assert_eq!(format_date(add_interval(d, -12, 0)), "1997-12-01");
    }

    #[test]
    fn take_and_nulls() {
        let mut b = ColumnBuilder::new(DataType::Str, 3);
        b.push(&Value::Str("a".into()));
        b.push_null();
        b.push(&Value::Str("c".into()));
        let c = b.finish();
        let t = c.take(&[2, 1, 0]);
        assert_eq!(t.get(0), Value::Str("c".into()));
        assert_eq!(t.get(1), Value::Null);
        assert_eq!(t.null_count(), 1);
    }
}
