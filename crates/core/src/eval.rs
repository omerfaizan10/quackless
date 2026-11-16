//! Vectorized expression evaluation: every expression evaluates a whole
//! batch at once and returns a column.

use crate::ast::BinOp;
use crate::exec::{run_plan_rows, ExecCtx};
use crate::expr::{ColRef, Expr};
use crate::plan::{expr_type, numeric_result, unify, LogicalPlan, Schema};
use crate::types::*;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SubKind {
    Scalar,
    Exists,
    In,
}

#[derive(Debug)]
pub struct SubqueryExpr {
    pub kind: SubKind,
    pub negated: bool,
    pub plan: LogicalPlan,
    pub probe: Option<Box<PExpr>>,
    /// Outer column references and where to find them in the input batch.
    pub outer: Vec<(ColRef, usize)>,
    pub cache: RefCell<HashMap<Vec<u8>, SubResult>>,
}

#[derive(Debug, Clone)]
pub enum SubResult {
    Scalar(Value),
    Exists(bool),
    Set { set: HashSet<Vec<u8>>, has_null: bool, dtype: DataType },
}

#[derive(Debug)]
pub enum PExpr {
    Col(usize),
    Lit(Value, DataType),
    Binary { op: BinOp, l: Box<PExpr>, r: Box<PExpr>, lt: DataType, rt: DataType },
    Neg(Box<PExpr>),
    Not(Box<PExpr>),
    IsNull { e: Box<PExpr>, negated: bool },
    Like { e: Box<PExpr>, pattern: LikePattern, negated: bool },
    LikeDyn { e: Box<PExpr>, pattern: Box<PExpr>, negated: bool },
    InSet { e: Box<PExpr>, set: HashSet<Vec<u8>>, has_null: bool, dtype: DataType, negated: bool },
    Case { operand: Option<Box<PExpr>>, whens: Vec<(PExpr, PExpr)>, else_: Option<Box<PExpr>>, dtype: DataType },
    Cast { e: Box<PExpr>, dtype: DataType },
    Func { name: String, args: Vec<PExpr>, dtype: DataType },
    Subquery(Box<SubqueryExpr>),
}

// ---------------------------------------------------------------------- compile

pub fn compile(e: &Expr, schema: &Schema) -> Result<PExpr, String> {
    Ok(match e {
        Expr::Column(c) => PExpr::Col(schema.index_of(c).ok_or_else(|| format!("internal: column {} not in input", c))?),
        Expr::Outer(c) => return Err(format!("internal: unbound outer reference {}", c)),
        Expr::Lit(v) => PExpr::Lit(v.clone(), v.data_type()),
        Expr::Binary { op, left, right } => {
            let lt = expr_type(left, schema, None)?;
            let rt = expr_type(right, schema, None)?;
            PExpr::Binary { op: *op, l: Box::new(compile(left, schema)?), r: Box::new(compile(right, schema)?), lt, rt }
        }
        Expr::Neg(x) => PExpr::Neg(Box::new(compile(x, schema)?)),
        Expr::Not(x) => PExpr::Not(Box::new(compile(x, schema)?)),
        Expr::IsNull { expr, negated } => PExpr::IsNull { e: Box::new(compile(expr, schema)?), negated: *negated },
        Expr::Like { expr, pattern, negated } => match pattern.as_ref() {
            Expr::Lit(Value::Str(p)) => {
                PExpr::Like { e: Box::new(compile(expr, schema)?), pattern: LikePattern::new(p), negated: *negated }
            }
            p => PExpr::LikeDyn { e: Box::new(compile(expr, schema)?), pattern: Box::new(compile(p, schema)?), negated: *negated },
        },
        Expr::InList { expr, list, negated } => {
            if list.iter().all(|x| matches!(x, Expr::Lit(_))) {
                let mut dt = expr_type(expr, schema, None)?;
                for x in list {
                    if let Expr::Lit(v) = x {
                        dt = unify(dt, v.data_type());
                    }
                }
                let mut set = HashSet::new();
                let mut has_null = false;
                for x in list {
                    if let Expr::Lit(v) = x {
                        if v.is_null() {
                            has_null = true;
                            continue;
                        }
                        let v = cast_value(v, dt)?;
                        let mut buf = Vec::new();
                        encode_value(&v, &mut buf);
                        set.insert(buf);
                    }
                }
                PExpr::InSet { e: Box::new(compile(expr, schema)?), set, has_null, dtype: dt, negated: *negated }
            } else {
                // x IN (a, b) == x = a OR x = b
                let ors = list
                    .iter()
                    .map(|x| Expr::binary(BinOp::Eq, (**expr).clone(), x.clone()))
                    .reduce(|a, b| Expr::binary(BinOp::Or, a, b))
                    .unwrap_or(Expr::Lit(Value::Bool(false)));
                let ors = if *negated { Expr::Not(Box::new(ors)) } else { ors };
                compile(&ors, schema)?
            }
        }
        Expr::Case { operand, whens, else_ } => {
            let dtype = expr_type(e, schema, None)?;
            PExpr::Case {
                operand: match operand {
                    Some(o) => Some(Box::new(compile(o, schema)?)),
                    None => None,
                },
                whens: whens.iter().map(|(a, b)| Ok((compile(a, schema)?, compile(b, schema)?))).collect::<Result<_, String>>()?,
                else_: match else_ {
                    Some(x) => Some(Box::new(compile(x, schema)?)),
                    None => None,
                },
                dtype,
            }
        }
        Expr::Cast { expr, dtype } => PExpr::Cast { e: Box::new(compile(expr, schema)?), dtype: *dtype },
        Expr::Func { name, args } => PExpr::Func {
            name: name.clone(),
            args: args.iter().map(|a| compile(a, schema)).collect::<Result<_, _>>()?,
            dtype: expr_type(e, schema, None)?,
        },
        Expr::Agg { .. } => return Err("internal: aggregate outside of aggregation".into()),
        Expr::ScalarSubquery(p) => subquery(SubKind::Scalar, false, p, None, schema)?,
        Expr::Exists { plan, negated } => subquery(SubKind::Exists, *negated, plan, None, schema)?,
        Expr::InSubquery { expr, plan, negated } => subquery(SubKind::In, *negated, plan, Some(compile(expr, schema)?), schema)?,
    })
}

fn subquery(kind: SubKind, negated: bool, plan: &LogicalPlan, probe: Option<PExpr>, schema: &Schema) -> Result<PExpr, String> {
    let mut outer = Vec::new();
    for c in plan.outer_refs() {
        let i = schema.index_of(&c).ok_or_else(|| format!("internal: correlated column {} not available", c))?;
        outer.push((c, i));
    }
    Ok(PExpr::Subquery(Box::new(SubqueryExpr {
        kind,
        negated,
        plan: plan.clone(),
        probe: probe.map(Box::new),
        outer,
        cache: RefCell::new(HashMap::new()),
    })))
}

// ---------------------------------------------------------------------- LIKE

#[derive(Debug, Clone)]
pub enum LikePattern {
    Exact(String),
    Prefix(String),
    Suffix(String),
    Contains(String),
    General(Vec<char>),
}

impl LikePattern {
    pub fn new(p: &str) -> Self {
        let has_us = p.contains('_');
        let inner = p.trim_matches('%');
        if !has_us && !inner.contains('%') {
            let starts = p.starts_with('%');
            let ends = p.ends_with('%') && p.len() > 0;
            return match (starts, ends) {
                (false, false) => LikePattern::Exact(p.to_string()),
                (false, true) => LikePattern::Prefix(inner.to_string()),
                (true, false) => LikePattern::Suffix(inner.to_string()),
                (true, true) => LikePattern::Contains(inner.to_string()),
            };
        }
        LikePattern::General(p.chars().collect())
    }
    pub fn matches(&self, s: &str) -> bool {
        match self {
            LikePattern::Exact(p) => s == p,
            LikePattern::Prefix(p) => s.starts_with(p.as_str()),
            LikePattern::Suffix(p) => s.ends_with(p.as_str()),
            LikePattern::Contains(p) => s.contains(p.as_str()),
            LikePattern::General(p) => like_general(s, p),
        }
    }
}

/// Glob match with `%` (any run) and `_` (one char), iterative with backtracking.
fn like_general(s: &str, p: &[char]) -> bool {
    let s: Vec<char> = s.chars().collect();
    let (mut si, mut pi) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '_' || p[pi] == s[si]) {
            si += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '%' {
            star = Some(pi);
            pi += 1;
            mark = si;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '%' {
        pi += 1;
    }
    pi == p.len()
}

// ---------------------------------------------------------------------- hashing keys

pub fn encode_value(v: &Value, buf: &mut Vec<u8>) {
    match v {
        Value::Null => buf.push(0),
        Value::Int(i) => {
            buf.push(1);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        Value::Float(f) => {
            buf.push(2);
            let f = if *f == 0.0 { 0.0 } else { *f };
            buf.extend_from_slice(&f.to_bits().to_le_bytes());
        }
        Value::Str(s) => {
            buf.push(3);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        Value::Date(d) => {
            buf.push(4);
            buf.extend_from_slice(&d.to_le_bytes());
        }
        Value::Bool(b) => {
            buf.push(5);
            buf.push(*b as u8);
        }
        Value::Interval { months, days } => {
            buf.push(6);
            buf.extend_from_slice(&months.to_le_bytes());
            buf.extend_from_slice(&days.to_le_bytes());
        }
    }
}

/// Same encoding as `encode_value`, read straight from a column.
#[inline]
pub fn encode_cell(c: &Column, i: usize, buf: &mut Vec<u8>) {
    if !c.is_valid(i) {
        buf.push(0);
        return;
    }
    match &c.data {
        ColumnData::Int(v) => {
            buf.push(1);
            buf.extend_from_slice(&v[i].to_le_bytes());
        }
        ColumnData::Float(v) => {
            buf.push(2);
            let f = if v[i] == 0.0 { 0.0 } else { v[i] };
            buf.extend_from_slice(&f.to_bits().to_le_bytes());
        }
        ColumnData::Str(v) => {
            let s = v.get(i);
            buf.push(3);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        ColumnData::Date(v) => {
            buf.push(4);
            buf.extend_from_slice(&v[i].to_le_bytes());
        }
        ColumnData::Bool(v) => {
            buf.push(5);
            buf.push(v[i] as u8);
        }
    }
}

// ---------------------------------------------------------------------- evaluation

fn and_validity(a: &Column, b: &Column) -> Option<Vec<bool>> {
    match (&a.validity, &b.validity) {
        (None, None) => None,
        (Some(x), None) | (None, Some(x)) => Some(x.clone()),
        (Some(x), Some(y)) => Some(x.iter().zip(y).map(|(p, q)| *p && *q).collect()),
    }
}

macro_rules! cmp_loop {
    ($op:expr, $n:expr, |$i:ident| $a:expr, $b:expr) => {{
        let n = $n;
        let mut out: Vec<bool> = Vec::with_capacity(n);
        match $op {
            BinOp::Eq => {
                for $i in 0..n {
                    out.push($a == $b)
                }
            }
            BinOp::NotEq => {
                for $i in 0..n {
                    out.push($a != $b)
                }
            }
            BinOp::Lt => {
                for $i in 0..n {
                    out.push($a < $b)
                }
            }
            BinOp::LtEq => {
                for $i in 0..n {
                    out.push($a <= $b)
                }
            }
            BinOp::Gt => {
                for $i in 0..n {
                    out.push($a > $b)
                }
            }
            BinOp::GtEq => {
                for $i in 0..n {
                    out.push($a >= $b)
                }
            }
            _ => unreachable!(),
        }
        out
    }};
}

fn as_f64(c: &Column) -> std::borrow::Cow<'_, [f64]> {
    match &c.data {
        ColumnData::Float(v) => std::borrow::Cow::Borrowed(v),
        ColumnData::Int(v) => std::borrow::Cow::Owned(v.iter().map(|x| *x as f64).collect()),
        ColumnData::Bool(v) => std::borrow::Cow::Owned(v.iter().map(|x| *x as i64 as f64).collect()),
        _ => std::borrow::Cow::Owned(vec![f64::NAN; c.len()]),
    }
}

fn compare(op: BinOp, l: &Column, r: &Column) -> Result<Column, String> {
    let n = l.len();
    let validity = and_validity(l, r);
    let out = match (&l.data, &r.data) {
        (ColumnData::Int(a), ColumnData::Int(b)) => cmp_loop!(op, n, |i| a[i], b[i]),
        (ColumnData::Str(a), ColumnData::Str(b)) => cmp_loop!(op, n, |i| a.get(i), b.get(i)),
        (ColumnData::Date(a), ColumnData::Date(b)) => cmp_loop!(op, n, |i| a[i], b[i]),
        (ColumnData::Bool(a), ColumnData::Bool(b)) => cmp_loop!(op, n, |i| a[i], b[i]),
        (ColumnData::Date(_), ColumnData::Str(_)) => return compare(op, l, &r.cast(DataType::Date)?),
        (ColumnData::Str(_), ColumnData::Date(_)) => return compare(op, &l.cast(DataType::Date)?, r),
        (ColumnData::Str(_), _) => return compare(op, l, &r.cast(DataType::Str)?),
        (_, ColumnData::Str(_)) => return compare(op, &l.cast(DataType::Str)?, r),
        _ => {
            let a = as_f64(l);
            let b = as_f64(r);
            cmp_loop!(op, n, |i| a[i], b[i])
        }
    };
    Ok(Column::with_validity(ColumnData::Bool(out), validity))
}

/// Column op scalar fast path for comparisons (the common WHERE shape).
fn compare_scalar(op: BinOp, l: &Column, v: &Value) -> Option<Column> {
    let n = l.len();
    let out = match (&l.data, v) {
        (ColumnData::Int(a), Value::Int(b)) => {
            let b = *b;
            cmp_loop!(op, n, |i| a[i], b)
        }
        (ColumnData::Float(a), Value::Float(b)) => {
            let b = *b;
            cmp_loop!(op, n, |i| a[i], b)
        }
        (ColumnData::Float(a), Value::Int(b)) => {
            let b = *b as f64;
            cmp_loop!(op, n, |i| a[i], b)
        }
        (ColumnData::Date(a), Value::Date(b)) => {
            let b = *b;
            cmp_loop!(op, n, |i| a[i], b)
        }
        (ColumnData::Str(a), Value::Str(b)) => {
            let b = b.as_str();
            cmp_loop!(op, n, |i| a.get(i), b)
        }
        _ => return None,
    };
    Some(Column::with_validity(ColumnData::Bool(out), l.validity.clone()))
}

fn arith(op: BinOp, l: &Column, r: &Column, int_div: bool) -> Result<Column, String> {
    let n = l.len();
    let mut validity = and_validity(l, r);
    let lt = l.data_type();
    let rt = r.data_type();
    // date arithmetic
    match (lt, rt, op) {
        (DataType::Date, DataType::Int, BinOp::Plus | BinOp::Minus) | (DataType::Int, DataType::Date, BinOp::Plus) => {
            let (d, k) = if lt == DataType::Date { (l, r) } else { (r, l) };
            let (ColumnData::Date(dv), ColumnData::Int(kv)) = (&d.data, &k.data) else { unreachable!() };
            let sign = if op == BinOp::Minus { -1 } else { 1 };
            let out = (0..n).map(|i| dv[i] + sign * kv[i] as i32).collect();
            return Ok(Column::with_validity(ColumnData::Date(out), validity));
        }
        (DataType::Date, DataType::Date, BinOp::Minus) => {
            let (ColumnData::Date(a), ColumnData::Date(b)) = (&l.data, &r.data) else { unreachable!() };
            let out = (0..n).map(|i| (a[i] - b[i]) as i64).collect();
            return Ok(Column::with_validity(ColumnData::Int(out), validity));
        }
        _ => {}
    }
    let rt2 = numeric_result(lt, rt);
    if !lt.is_numeric() && lt != DataType::Bool || !rt.is_numeric() && rt != DataType::Bool {
        return Err(format!("Binder error: cannot apply {} to {} and {}", op.symbol(), lt.name(), rt.name()));
    }
    let float_div = op == BinOp::Div && !(int_div && rt2 == DataType::Int);
    if rt2 == DataType::Int && !float_div {
        let (ColumnData::Int(a), ColumnData::Int(b)) = (&l.data, &r.data) else {
            return arith(op, &l.cast(DataType::Int)?, &r.cast(DataType::Int)?, int_div);
        };
        let mut out = Vec::with_capacity(n);
        match op {
            BinOp::Plus => out.extend((0..n).map(|i| a[i].wrapping_add(b[i]))),
            BinOp::Minus => out.extend((0..n).map(|i| a[i].wrapping_sub(b[i]))),
            BinOp::Mul => out.extend((0..n).map(|i| a[i].wrapping_mul(b[i]))),
            BinOp::Div | BinOp::Mod => {
                let mut v = validity.unwrap_or_else(|| vec![true; n]);
                for i in 0..n {
                    if b[i] == 0 {
                        // SQLite dialect: NULL. DuckDB dialect: an error for a non-NULL row.
                        if !int_div && v[i] {
                            return Err(format!("Invalid Input Error: Division by zero in expression ({} {} 0)", a[i], op.symbol()));
                        }
                        v[i] = false;
                        out.push(0);
                    } else if op == BinOp::Div {
                        out.push(a[i].wrapping_div(b[i]));
                    } else {
                        out.push(a[i].wrapping_rem(b[i]));
                    }
                }
                validity = Some(v);
            }
            _ => unreachable!(),
        }
        return Ok(Column::with_validity(ColumnData::Int(out), validity));
    }
    let a = as_f64(l);
    let b = as_f64(r);
    let mut out = Vec::with_capacity(n);
    match op {
        BinOp::Plus => out.extend((0..n).map(|i| a[i] + b[i])),
        BinOp::Minus => out.extend((0..n).map(|i| a[i] - b[i])),
        BinOp::Mul => out.extend((0..n).map(|i| a[i] * b[i])),
        BinOp::Div | BinOp::Mod => {
            let mut v = validity.unwrap_or_else(|| vec![true; n]);
            for i in 0..n {
                if b[i] == 0.0 && !int_div && op == BinOp::Div {
                    // DuckDB dialect: IEEE division (inf, -inf, NaN)
                    out.push(a[i] / b[i]);
                } else if b[i] == 0.0 {
                    if !int_div && v[i] {
                        return Err(format!("Invalid Input Error: Division by zero in expression ({} % 0)", a[i]));
                    }
                    v[i] = false;
                    out.push(0.0);
                } else if op == BinOp::Div {
                    out.push(a[i] / b[i]);
                } else {
                    out.push(a[i] % b[i]);
                }
            }
            validity = Some(v);
        }
        _ => unreachable!(),
    }
    Ok(Column::with_validity(ColumnData::Float(out), validity))
}

fn bool_vec(c: &Column) -> Result<&Vec<bool>, String> {
    match &c.data {
        ColumnData::Bool(v) => Ok(v),
        _ => Err(format!("expected BOOLEAN, got {}", c.data_type().name())),
    }
}

fn to_bool(c: Column) -> Result<Column, String> {
    if c.data_type() == DataType::Bool {
        Ok(c)
    } else {
        c.cast(DataType::Bool)
    }
}

/// Three-valued AND / OR.
fn logical(op: BinOp, l: &Column, r: &Column) -> Result<Column, String> {
    let a = bool_vec(l)?;
    let b = bool_vec(r)?;
    let n = a.len();
    if l.validity.is_none() && r.validity.is_none() {
        let out = if op == BinOp::And {
            (0..n).map(|i| a[i] && b[i]).collect()
        } else {
            (0..n).map(|i| a[i] || b[i]).collect()
        };
        return Ok(Column::new(ColumnData::Bool(out)));
    }
    let mut out = Vec::with_capacity(n);
    let mut valid = Vec::with_capacity(n);
    for i in 0..n {
        let x = if l.is_valid(i) { Some(a[i]) } else { None };
        let y = if r.is_valid(i) { Some(b[i]) } else { None };
        let res = if op == BinOp::And {
            match (x, y) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            }
        } else {
            match (x, y) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            }
        };
        out.push(res.unwrap_or(false));
        valid.push(res.is_some());
    }
    Ok(Column::with_validity(ColumnData::Bool(out), Some(valid)))
}

pub fn eval(e: &PExpr, b: &Batch, ctx: &ExecCtx) -> Result<Column, String> {
    let n = b.rows;
    Ok(match e {
        PExpr::Col(i) => b.columns[*i].clone(),
        PExpr::Lit(v, dt) => Column::repeat(v, *dt, n),
        PExpr::Binary { op, l, r, .. } => {
            if op.is_comparison() {
                if let PExpr::Lit(v, _) = r.as_ref() {
                    let lc = eval(l, b, ctx)?;
                    if v.is_null() {
                        return Ok(Column::nulls(DataType::Bool, n));
                    }
                    if let Some(c) = compare_scalar(*op, &lc, v) {
                        return Ok(c);
                    }
                    let rc = Column::repeat(v, v.data_type(), n);
                    return compare(*op, &lc, &rc);
                }
                let lc = eval(l, b, ctx)?;
                let rc = eval(r, b, ctx)?;
                return compare(*op, &lc, &rc);
            }
            match op {
                BinOp::And | BinOp::Or => {
                    let lc = to_bool(eval(l, b, ctx)?)?;
                    let rc = to_bool(eval(r, b, ctx)?)?;
                    logical(*op, &lc, &rc)?
                }
                BinOp::Concat => {
                    let lc = eval(l, b, ctx)?;
                    let rc = eval(r, b, ctx)?;
                    let mut bld = ColumnBuilder::new(DataType::Str, n);
                    for i in 0..n {
                        if lc.is_valid(i) && rc.is_valid(i) {
                            bld.push(&Value::Str(format!("{}{}", lc.get(i), rc.get(i))));
                        } else {
                            bld.push_null();
                        }
                    }
                    bld.finish()
                }
                _ => {
                    let lc = eval(l, b, ctx)?;
                    let rc = eval(r, b, ctx)?;
                    if let PExpr::Lit(Value::Interval { months, days }, _) = r.as_ref() {
                        return date_interval(&lc, *months, *days, *op == BinOp::Minus);
                    }
                    if let PExpr::Lit(Value::Interval { months, days }, _) = l.as_ref() {
                        return date_interval(&rc, *months, *days, false);
                    }
                    if lc.data_type() == DataType::Str || rc.data_type() == DataType::Str {
                        // numeric strings in arithmetic (SQLite-style leniency is not offered)
                        return Err(format!("Binder error: cannot apply {} to VARCHAR", op.symbol()));
                    }
                    arith(*op, &lc, &rc, ctx.int_div)?
                }
            }
        }
        PExpr::Neg(x) => {
            let c = eval(x, b, ctx)?;
            let data = match &c.data {
                ColumnData::Int(v) => ColumnData::Int(v.iter().map(|x| x.wrapping_neg()).collect()),
                ColumnData::Float(v) => ColumnData::Float(v.iter().map(|x| -x).collect()),
                _ => return Err("cannot negate a non-numeric value".into()),
            };
            Column::with_validity(data, c.validity.clone())
        }
        PExpr::Not(x) => {
            let c = to_bool(eval(x, b, ctx)?)?;
            let v = bool_vec(&c)?.iter().map(|x| !x).collect();
            Column::with_validity(ColumnData::Bool(v), c.validity.clone())
        }
        PExpr::IsNull { e, negated } => {
            let c = eval(e, b, ctx)?;
            Column::new(ColumnData::Bool((0..n).map(|i| c.is_valid(i) == *negated).collect()))
        }
        PExpr::Like { e, pattern, negated } => {
            let c = eval(e, b, ctx)?;
            let c = if c.data_type() == DataType::Str { c } else { c.cast(DataType::Str)? };
            let ColumnData::Str(s) = &c.data else { unreachable!() };
            let out = (0..n).map(|i| pattern.matches(s.get(i)) != *negated).collect();
            Column::with_validity(ColumnData::Bool(out), c.validity.clone())
        }
        PExpr::LikeDyn { e, pattern, negated } => {
            let c = eval(e, b, ctx)?.cast(DataType::Str)?;
            let p = eval(pattern, b, ctx)?.cast(DataType::Str)?;
            let mut bld = ColumnBuilder::new(DataType::Bool, n);
            for i in 0..n {
                if c.is_valid(i) && p.is_valid(i) {
                    let (Value::Str(s), Value::Str(pp)) = (c.get(i), p.get(i)) else { unreachable!() };
                    bld.push(&Value::Bool(LikePattern::new(&pp).matches(&s) != *negated));
                } else {
                    bld.push_null();
                }
            }
            bld.finish()
        }
        PExpr::InSet { e, set, has_null, dtype, negated } => {
            let c = eval(e, b, ctx)?;
            let c = if c.data_type() == *dtype || set.is_empty() { c } else { c.cast(*dtype)? };
            in_set(&c, set, *has_null, *negated)
        }
        PExpr::Case { operand, whens, else_, dtype } => {
            // Selective evaluation: each condition runs only on rows no earlier WHEN
            // decided, and each THEN / ELSE only on the rows it produces. Errors in a
            // branch a row never reaches (division by zero) therefore do not fire.
            let dt = if *dtype == DataType::Null { DataType::Int } else { *dtype };
            let op = match operand {
                Some(o) => Some(eval(o, b, ctx)?),
                None => None,
            };
            let mut remaining: Vec<u32> = (0..n as u32).collect();
            // (rows, values) per branch that produced at least one row
            let mut parts: Vec<(Vec<u32>, Column)> = Vec::new();
            for (cond, val) in whens.iter() {
                if remaining.is_empty() {
                    break;
                }
                let sub = sub_batch(b, &remaining, n);
                let cv = eval(cond, &sub, ctx)?;
                let c = match &op {
                    Some(o) => compare(BinOp::Eq, take_rows(o, &remaining, n).as_ref(), &cv)?,
                    None => to_bool(cv)?,
                };
                let bits = bool_vec(&c)?;
                let mut hit = Vec::new();
                let mut miss = Vec::new();
                for (j, &row) in remaining.iter().enumerate() {
                    if c.is_valid(j) && bits[j] {
                        hit.push(row);
                    } else {
                        miss.push(row);
                    }
                }
                if !hit.is_empty() {
                    let v = eval(val, &sub_batch(b, &hit, n), ctx)?;
                    parts.push((hit, v));
                }
                remaining = miss;
            }
            if !remaining.is_empty() {
                if let Some(x) = else_ {
                    let v = eval(x, &sub_batch(b, &remaining, n), ctx)?;
                    parts.push((remaining, v));
                }
            }
            scatter(dt, n, parts)
        }
        PExpr::Func { name, args, dtype } if name == "coalesce" && !args.is_empty() => {
            // arguments after the first are evaluated only where everything before was NULL
            let dt = if *dtype == DataType::Null { DataType::Int } else { *dtype };
            let mut remaining: Vec<u32> = (0..n as u32).collect();
            let mut parts: Vec<(Vec<u32>, Column)> = Vec::new();
            for a in args {
                if remaining.is_empty() {
                    break;
                }
                let v = eval(a, &sub_batch(b, &remaining, n), ctx)?.cast(dt)?;
                let mut got = Vec::new();
                let mut got_idx = Vec::new();
                let mut miss = Vec::new();
                for (j, &row) in remaining.iter().enumerate() {
                    if v.is_valid(j) {
                        got.push(row);
                        got_idx.push(j as u32);
                    } else {
                        miss.push(row);
                    }
                }
                if !got.is_empty() {
                    let vals = if got.len() == remaining.len() { v } else { v.take(&got_idx) };
                    parts.push((got, vals));
                }
                remaining = miss;
            }
            scatter(dt, n, parts)
        }
        PExpr::Cast { e, dtype } => eval(e, b, ctx)?.cast(*dtype)?,
        PExpr::Func { name, args, dtype } => {
            let cols: Vec<Column> = args.iter().map(|a| eval(a, b, ctx)).collect::<Result<_, _>>()?;
            crate::functions::call(name, &cols, *dtype, n)?
        }
        PExpr::Subquery(sq) => eval_subquery(sq, b, ctx)?,
    })
}

/// The rows `rows` of a batch (no copy when that is every row).
fn sub_batch<'b>(b: &'b Batch, rows: &[u32], n: usize) -> Cow<'b, Batch> {
    if rows.len() == n {
        Cow::Borrowed(b)
    } else {
        Cow::Owned(b.take(rows))
    }
}

fn take_rows<'c>(c: &'c Column, rows: &[u32], n: usize) -> Cow<'c, Column> {
    if rows.len() == n {
        Cow::Borrowed(c)
    } else {
        Cow::Owned(c.take(rows))
    }
}

/// Assemble a column of `n` rows from per-branch pieces; rows in no piece are NULL.
fn scatter(dt: DataType, n: usize, mut parts: Vec<(Vec<u32>, Column)>) -> Column {
    if parts.len() == 1 && parts[0].0.len() == n {
        if parts[0].1.data_type() == dt {
            return parts.pop().unwrap().1;
        }
        if let Ok(c) = parts[0].1.cast(dt) {
            return c;
        }
    }
    let mut src: Vec<Option<(usize, usize)>> = vec![None; n];
    for (k, (rows, _)) in parts.iter().enumerate() {
        for (j, &r) in rows.iter().enumerate() {
            src[r as usize] = Some((k, j));
        }
    }
    let cast: Vec<Column> = parts.iter().map(|(_, c)| c.cast(dt).unwrap_or_else(|_| c.clone())).collect();
    let mut bld = ColumnBuilder::new(dt, n);
    for s in src {
        match s {
            Some((k, j)) => bld.push_from(&cast[k], j),
            None => bld.push_null(),
        }
    }
    bld.finish()
}

fn date_interval(c: &Column, months: i32, days: i32, minus: bool) -> Result<Column, String> {
    let (m, d) = if minus { (-months, -days) } else { (months, days) };
    match &c.data {
        ColumnData::Date(v) => Ok(Column::with_validity(
            ColumnData::Date(v.iter().map(|x| add_interval(*x, m, d)).collect()),
            c.validity.clone(),
        )),
        _ => Err("interval arithmetic requires a DATE".into()),
    }
}

fn in_set(c: &Column, set: &HashSet<Vec<u8>>, has_null: bool, negated: bool) -> Column {
    let n = c.len();
    let mut out = Vec::with_capacity(n);
    let mut valid = Vec::with_capacity(n);
    let mut buf = Vec::with_capacity(32);
    for i in 0..n {
        if !c.is_valid(i) {
            // NULL IN (empty set) is false, otherwise NULL
            out.push(negated);
            valid.push(set.is_empty() && !has_null);
            continue;
        }
        buf.clear();
        encode_cell(c, i, &mut buf);
        let found = set.contains(&buf);
        if found {
            out.push(!negated);
            valid.push(true);
        } else if has_null {
            out.push(false);
            valid.push(false);
        } else {
            out.push(negated);
            valid.push(true);
        }
    }
    Column::with_validity(ColumnData::Bool(out), Some(valid))
}

fn substitute_outer(plan: &LogicalPlan, vals: &HashMap<ColRef, Value>) -> Result<LogicalPlan, String> {
    plan.clone().transform_up(&mut |node| {
        node.map_exprs(&mut |e| {
            e.transform(&mut |x| match x {
                Expr::Outer(c) => Ok(Expr::Lit(vals.get(&c).cloned().unwrap_or(Value::Null))),
                other => Ok(other),
            })
        })
    })
}

fn run_sub(sq: &SubqueryExpr, plan: &LogicalPlan, probe_type: DataType, ctx: &ExecCtx) -> Result<SubResult, String> {
    let (cols, rows) = run_plan_rows(plan, ctx, if sq.kind == SubKind::Exists { Some(1) } else { None })?;
    Ok(match sq.kind {
        SubKind::Exists => SubResult::Exists(rows > 0),
        SubKind::Scalar => {
            if rows > 1 {
                return Err("Invalid Input Error: More than one row returned by a subquery used as an expression".into());
            }
            SubResult::Scalar(if rows == 0 { Value::Null } else { cols[0].get(0) })
        }
        SubKind::In => {
            let dtype = unify(probe_type, cols[0].data_type());
            let c = cols[0].cast(dtype)?;
            let mut set = HashSet::new();
            let mut has_null = false;
            for i in 0..rows {
                if !c.is_valid(i) {
                    has_null = true;
                    continue;
                }
                let mut buf = Vec::new();
                encode_cell(&c, i, &mut buf);
                set.insert(buf);
            }
            SubResult::Set { set, has_null, dtype }
        }
    })
}

fn get_result(sq: &SubqueryExpr, key: &[u8], plan: impl FnOnce() -> Result<LogicalPlan, String>, probe_type: DataType, ctx: &ExecCtx) -> Result<SubResult, String> {
    if let Some(r) = sq.cache.borrow().get(key) {
        return Ok(r.clone());
    }
    let p = plan()?;
    let r = run_sub(sq, &p, probe_type, ctx)?;
    sq.cache.borrow_mut().insert(key.to_vec(), r.clone());
    Ok(r)
}

fn eval_subquery(sq: &SubqueryExpr, b: &Batch, ctx: &ExecCtx) -> Result<Column, String> {
    let n = b.rows;
    let probe = match &sq.probe {
        Some(p) => Some(eval(p, b, ctx)?),
        None => None,
    };
    let probe_type = probe.as_ref().map(|p| p.data_type()).unwrap_or(DataType::Null);
    let out_type = match sq.kind {
        SubKind::Scalar => sq.plan.schema().fields[0].dtype,
        _ => DataType::Bool,
    };
    if sq.outer.is_empty() {
        // uncorrelated: run once, then evaluate vectorized
        let r = get_result(sq, &[], || Ok(sq.plan.clone()), probe_type, ctx)?;
        return Ok(match r {
            SubResult::Scalar(v) => Column::repeat(&v, out_type, n),
            SubResult::Exists(e) => Column::repeat(&Value::Bool(e != sq.negated), DataType::Bool, n),
            SubResult::Set { set, has_null, dtype } => {
                let p = probe.unwrap();
                let p = if p.data_type() == dtype { p } else { p.cast(dtype)? };
                in_set(&p, &set, has_null, sq.negated)
            }
        });
    }
    let mut bld = ColumnBuilder::new(out_type, n);
    let mut key = Vec::new();
    for i in 0..n {
        key.clear();
        for (_, idx) in &sq.outer {
            encode_cell(&b.columns[*idx], i, &mut key);
        }
        let make_plan = || {
            let vals: HashMap<ColRef, Value> = sq.outer.iter().map(|(c, idx)| (c.clone(), b.columns[*idx].get(i))).collect();
            substitute_outer(&sq.plan, &vals)
        };
        match get_result(sq, &key, make_plan, probe_type, ctx)? {
            SubResult::Scalar(v) => bld.push(&v),
            SubResult::Exists(e) => bld.push(&Value::Bool(e != sq.negated)),
            SubResult::Set { set, has_null, dtype } => {
                let one = probe.as_ref().unwrap().slice(i, 1).cast(dtype)?;
                let r = in_set(&one, &set, has_null, sq.negated);
                bld.push_from(&r, 0);
            }
        }
    }
    Ok(bld.finish())
}
