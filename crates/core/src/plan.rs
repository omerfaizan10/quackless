//! Logical plan, schemas and expression typing.

use crate::ast::BinOp;
use crate::expr::{AggFunc, ColRef, Expr};
use crate::types::{DataType, Value};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub rel: Option<String>,
    pub name: String,
    pub dtype: DataType,
}

impl Field {
    pub fn new(rel: Option<&str>, name: &str, dtype: DataType) -> Self {
        Field { rel: rel.map(|s| s.to_string()), name: name.to_string(), dtype }
    }
    pub fn colref(&self) -> ColRef {
        ColRef { rel: self.rel.clone(), name: self.name.clone() }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub fields: Vec<Field>,
}

impl Schema {
    pub fn new(fields: Vec<Field>) -> Self {
        Schema { fields }
    }
    pub fn len(&self) -> usize {
        self.fields.len()
    }
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
    /// Resolve a possibly unqualified name. Errors on ambiguity, Ok(None) if absent.
    pub fn resolve(&self, rel: Option<&str>, name: &str) -> Result<Option<usize>, String> {
        let mut found = None;
        for (i, f) in self.fields.iter().enumerate() {
            let rel_ok = match rel {
                None => true,
                Some(r) => f.rel.as_deref() == Some(r),
            };
            if rel_ok && f.name == name {
                if found.is_some() {
                    return Err(format!("Binder error: ambiguous column reference \"{}\"", name));
                }
                found = Some(i);
            }
        }
        Ok(found)
    }
    /// Exact lookup of a resolved column reference.
    pub fn index_of(&self, c: &ColRef) -> Option<usize> {
        let mut hit = None;
        for (i, f) in self.fields.iter().enumerate() {
            if f.name == c.name && f.rel == c.rel {
                hit = Some(i);
                break;
            }
        }
        hit
    }
    pub fn contains(&self, c: &ColRef) -> bool {
        self.index_of(c).is_some()
    }
    pub fn join(&self, other: &Schema) -> Schema {
        let mut fields = self.fields.clone();
        fields.extend(other.fields.iter().cloned());
        Schema { fields }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Semi,
    Anti,
    Cross,
}

impl JoinType {
    pub fn name(self) -> &'static str {
        match self {
            JoinType::Inner => "INNER",
            JoinType::Left => "LEFT",
            JoinType::Semi => "SEMI",
            JoinType::Anti => "ANTI",
            JoinType::Cross => "CROSS",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    pub expr: Expr,
    pub asc: bool,
    pub nulls_first: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    Scan { table: String, alias: String, schema: Schema, projection: Option<Vec<usize>> },
    Values { schema: Schema, rows: Vec<Vec<Expr>> },
    Filter { input: Box<LogicalPlan>, predicate: Expr },
    Project { input: Box<LogicalPlan>, exprs: Vec<Expr>, schema: Schema },
    Aggregate { input: Box<LogicalPlan>, group: Vec<Expr>, aggs: Vec<Expr>, schema: Schema },
    Join { left: Box<LogicalPlan>, right: Box<LogicalPlan>, kind: JoinType, on: Vec<(Expr, Expr)>, filter: Option<Expr> },
    Sort { input: Box<LogicalPlan>, keys: Vec<SortKey> },
    Limit { input: Box<LogicalPlan>, limit: Option<usize>, offset: usize },
    /// Re-qualifies every output column (derived tables, CTE references, decorrelated subqueries).
    Alias { input: Box<LogicalPlan>, alias: String, schema: Schema },
    Union { left: Box<LogicalPlan>, right: Box<LogicalPlan>, all: bool, schema: Schema },
    /// EXCEPT / INTERSECT (set semantics).
    SetOp { left: Box<LogicalPlan>, right: Box<LogicalPlan>, intersect: bool, schema: Schema },
}

impl LogicalPlan {
    pub fn schema(&self) -> Schema {
        match self {
            LogicalPlan::Scan { schema, projection, .. } => match projection {
                None => schema.clone(),
                Some(p) => Schema::new(p.iter().map(|&i| schema.fields[i].clone()).collect()),
            },
            LogicalPlan::Values { schema, .. }
            | LogicalPlan::Project { schema, .. }
            | LogicalPlan::Aggregate { schema, .. }
            | LogicalPlan::Alias { schema, .. }
            | LogicalPlan::Union { schema, .. }
            | LogicalPlan::SetOp { schema, .. } => schema.clone(),
            LogicalPlan::Filter { input, .. } | LogicalPlan::Sort { input, .. } | LogicalPlan::Limit { input, .. } => {
                input.schema()
            }
            LogicalPlan::Join { left, right, kind, .. } => match kind {
                JoinType::Semi | JoinType::Anti => left.schema(),
                _ => left.schema().join(&right.schema()),
            },
        }
    }

    pub fn children(&self) -> Vec<&LogicalPlan> {
        match self {
            LogicalPlan::Scan { .. } | LogicalPlan::Values { .. } => vec![],
            LogicalPlan::Filter { input, .. }
            | LogicalPlan::Project { input, .. }
            | LogicalPlan::Aggregate { input, .. }
            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::Alias { input, .. } => vec![input],
            LogicalPlan::Join { left, right, .. }
            | LogicalPlan::Union { left, right, .. }
            | LogicalPlan::SetOp { left, right, .. } => vec![left, right],
        }
    }

    /// Every expression held directly by this node.
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            LogicalPlan::Filter { predicate, .. } => vec![predicate],
            LogicalPlan::Project { exprs, .. } => exprs.iter().collect(),
            LogicalPlan::Aggregate { group, aggs, .. } => group.iter().chain(aggs.iter()).collect(),
            LogicalPlan::Join { on, filter, .. } => {
                let mut v: Vec<&Expr> = on.iter().flat_map(|(a, b)| [a, b]).collect();
                if let Some(f) = filter {
                    v.push(f);
                }
                v
            }
            LogicalPlan::Sort { keys, .. } => keys.iter().map(|k| &k.expr).collect(),
            LogicalPlan::Values { rows, .. } => rows.iter().flatten().collect(),
            _ => vec![],
        }
    }

    /// True if any expression in the tree references an outer (correlated) column.
    pub fn has_outer_refs(&self) -> bool {
        let mut found = false;
        for e in self.exprs() {
            e.walk(&mut |x| match x {
                Expr::Outer(_) => found = true,
                Expr::ScalarSubquery(p) | Expr::Exists { plan: p, .. } | Expr::InSubquery { plan: p, .. } => {
                    if p.outer_refs_reaching_parent() {
                        found = true
                    }
                }
                _ => {}
            });
        }
        found || self.children().iter().any(|c| c.has_outer_refs())
    }

    /// Outer (correlated) column references anywhere in this plan, one level deep.
    pub fn outer_refs(&self) -> std::collections::BTreeSet<ColRef> {
        let mut out = std::collections::BTreeSet::new();
        self.collect_outer(&mut out);
        out
    }

    fn collect_outer(&self, out: &mut std::collections::BTreeSet<ColRef>) {
        for e in self.exprs() {
            e.walk(&mut |x| {
                if let Expr::Outer(c) = x {
                    out.insert(c.clone());
                }
            });
        }
        for c in self.children() {
            c.collect_outer(out);
        }
    }

    /// Conservative: a nested subquery that itself has outer refs might reach up two levels.
    /// I only bind outer references one level up, so nested refs never reach our parent.
    fn outer_refs_reaching_parent(&self) -> bool {
        false
    }

    pub fn node_label(&self) -> (String, String) {
        match self {
            LogicalPlan::Scan { table, alias, projection, schema } => {
                let cols = match projection {
                    Some(p) => p.iter().map(|&i| schema.fields[i].name.clone()).collect::<Vec<_>>().join(", "),
                    None => "*".into(),
                };
                let name = if alias != table { format!("{} AS {}", table, alias) } else { table.clone() };
                ("SCAN".into(), format!("{} [{}]", name, cols))
            }
            LogicalPlan::Values { rows, .. } => ("VALUES".into(), format!("{} row(s)", rows.len())),
            LogicalPlan::Filter { predicate, .. } => ("FILTER".into(), predicate.to_string()),
            LogicalPlan::Project { exprs, schema, .. } => (
                "PROJECT".into(),
                exprs
                    .iter()
                    .zip(&schema.fields)
                    .map(|(e, f)| {
                        let s = e.to_string();
                        if s == f.name || f.name.starts_with('#') { s } else { format!("{} AS {}", s, f.name) }
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            LogicalPlan::Aggregate { group, aggs, .. } => {
                let g = group.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ");
                let a = aggs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ");
                let mut d = String::new();
                if !group.is_empty() {
                    let _ = write!(d, "group by {}", g);
                }
                if !aggs.is_empty() {
                    if !d.is_empty() {
                        d.push_str("; ");
                    }
                    d.push_str(&a);
                }
                ((if group.is_empty() { "AGGREGATE" } else { "HASH_GROUP_BY" }).into(), d)
            }
            LogicalPlan::Join { kind, on, filter, .. } => {
                let mut parts: Vec<String> = on.iter().map(|(a, b)| format!("{} = {}", a, b)).collect();
                if let Some(f) = filter {
                    parts.push(f.to_string());
                }
                let op = if *kind == JoinType::Cross {
                    "CROSS_PRODUCT".to_string()
                } else if on.is_empty() {
                    format!("NESTED_LOOP_JOIN {}", kind.name())
                } else {
                    format!("HASH_JOIN {}", kind.name())
                };
                (op, parts.join(" AND "))
            }
            LogicalPlan::Sort { keys, .. } => (
                "ORDER_BY".into(),
                keys.iter()
                    .map(|k| format!("{}{}", k.expr, if k.asc { "" } else { " DESC" }))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            LogicalPlan::Limit { limit, offset, .. } => (
                "LIMIT".into(),
                match (limit, offset) {
                    (Some(l), 0) => format!("{}", l),
                    (Some(l), o) => format!("{} offset {}", l, o),
                    (None, o) => format!("offset {}", o),
                },
            ),
            LogicalPlan::Alias { alias, .. } => ("ALIAS".into(), alias.trim_start_matches('_').to_string()),
            LogicalPlan::Union { all, .. } => ((if *all { "UNION_ALL" } else { "UNION" }).into(), String::new()),
            LogicalPlan::SetOp { intersect, .. } => ((if *intersect { "INTERSECT" } else { "EXCEPT" }).into(), String::new()),
        }
    }

    /// Indented text rendering used by EXPLAIN and the CLI.
    pub fn display_tree(&self) -> String {
        let mut s = String::new();
        self.fmt_tree(&mut s, 0);
        s
    }

    fn fmt_tree(&self, s: &mut String, depth: usize) {
        let (op, detail) = self.node_label();
        let _ = writeln!(s, "{}{} {}", "  ".repeat(depth), op, detail);
        for c in self.children() {
            c.fmt_tree(s, depth + 1);
        }
    }

    /// Rewrite every expression in this node (not children).
    pub fn map_exprs(self, f: &mut dyn FnMut(Expr) -> Result<Expr, String>) -> Result<LogicalPlan, String> {
        Ok(match self {
            LogicalPlan::Filter { input, predicate } => LogicalPlan::Filter { input, predicate: f(predicate)? },
            LogicalPlan::Project { input, exprs, schema } => LogicalPlan::Project {
                input,
                exprs: exprs.into_iter().map(&mut *f).collect::<Result<_, _>>()?,
                schema,
            },
            LogicalPlan::Aggregate { input, group, aggs, schema } => LogicalPlan::Aggregate {
                input,
                group: group.into_iter().map(&mut *f).collect::<Result<_, _>>()?,
                aggs: aggs.into_iter().map(&mut *f).collect::<Result<_, _>>()?,
                schema,
            },
            LogicalPlan::Join { left, right, kind, on, filter } => LogicalPlan::Join {
                left,
                right,
                kind,
                on: on.into_iter().map(|(a, b)| Ok((f(a)?, f(b)?))).collect::<Result<_, String>>()?,
                filter: match filter {
                    Some(x) => Some(f(x)?),
                    None => None,
                },
            },
            LogicalPlan::Sort { input, keys } => LogicalPlan::Sort {
                input,
                keys: keys
                    .into_iter()
                    .map(|k| Ok(SortKey { expr: f(k.expr)?, asc: k.asc, nulls_first: k.nulls_first }))
                    .collect::<Result<_, String>>()?,
            },
            LogicalPlan::Values { schema, rows } => LogicalPlan::Values {
                schema,
                rows: rows
                    .into_iter()
                    .map(|r| r.into_iter().map(&mut *f).collect::<Result<_, _>>())
                    .collect::<Result<_, _>>()?,
            },
            other => other,
        })
    }

    /// Rebuild this node with new children (same order as `children()`).
    pub fn with_children(self, mut kids: Vec<LogicalPlan>) -> LogicalPlan {
        let mut next = || Box::new(kids.remove(0));
        match self {
            LogicalPlan::Filter { predicate, .. } => LogicalPlan::Filter { input: next(), predicate },
            LogicalPlan::Project { exprs, schema, .. } => LogicalPlan::Project { input: next(), exprs, schema },
            LogicalPlan::Aggregate { group, aggs, schema, .. } => {
                LogicalPlan::Aggregate { input: next(), group, aggs, schema }
            }
            LogicalPlan::Sort { keys, .. } => LogicalPlan::Sort { input: next(), keys },
            LogicalPlan::Limit { limit, offset, .. } => LogicalPlan::Limit { input: next(), limit, offset },
            LogicalPlan::Alias { alias, schema, .. } => LogicalPlan::Alias { input: next(), alias, schema },
            LogicalPlan::Join { kind, on, filter, .. } => {
                let l = next();
                let r = next();
                LogicalPlan::Join { left: l, right: r, kind, on, filter }
            }
            LogicalPlan::Union { all, schema, .. } => {
                let l = next();
                let r = next();
                LogicalPlan::Union { left: l, right: r, all, schema }
            }
            LogicalPlan::SetOp { intersect, schema, .. } => {
                let l = next();
                let r = next();
                LogicalPlan::SetOp { left: l, right: r, intersect, schema }
            }
            leaf => leaf,
        }
    }

    /// Bottom-up plan rewrite.
    pub fn transform_up(self, f: &mut dyn FnMut(LogicalPlan) -> Result<LogicalPlan, String>) -> Result<LogicalPlan, String> {
        let kids: Vec<LogicalPlan> = self.children().into_iter().cloned().collect();
        let mut new_kids = Vec::with_capacity(kids.len());
        for k in kids {
            new_kids.push(k.transform_up(f)?);
        }
        let node = if new_kids.is_empty() { self } else { self.with_children(new_kids) };
        f(node)
    }
}

// ---------------------------------------------------------------------------
// Expression typing
// ---------------------------------------------------------------------------

pub fn numeric_result(l: DataType, r: DataType) -> DataType {
    match (l, r) {
        (DataType::Int, DataType::Int) => DataType::Int,
        (DataType::Null, x) | (x, DataType::Null) if x.is_numeric() => x,
        (DataType::Null, DataType::Null) => DataType::Int,
        _ => DataType::Float,
    }
}

pub fn expr_type(e: &Expr, schema: &Schema, outer: Option<&Schema>) -> Result<DataType, String> {
    Ok(match e {
        Expr::Column(c) => match schema.index_of(c) {
            Some(i) => schema.fields[i].dtype,
            None => return Err(format!("Binder error: column {} not found", c)),
        },
        Expr::Outer(c) => match outer.and_then(|o| o.index_of(c).map(|i| o.fields[i].dtype)) {
            Some(t) => t,
            None => DataType::Null,
        },
        Expr::Lit(v) => v.data_type(),
        Expr::Binary { op, left, right } => {
            let l = expr_type(left, schema, outer)?;
            let r = expr_type(right, schema, outer)?;
            match op {
                BinOp::And | BinOp::Or => DataType::Bool,
                o if o.is_comparison() => DataType::Bool,
                BinOp::Concat => DataType::Str,
                BinOp::Div => {
                    if l == DataType::Interval || r == DataType::Interval {
                        DataType::Interval
                    } else {
                        // `/` always produces DOUBLE (DuckDB semantics); the
                        // integer-division dialect flag is handled at execution.
                        DataType::Float
                    }
                }
                BinOp::Plus | BinOp::Minus => match (l, r) {
                    (DataType::Date, DataType::Interval) | (DataType::Interval, DataType::Date) => DataType::Date,
                    (DataType::Date, DataType::Int) | (DataType::Int, DataType::Date) => DataType::Date,
                    (DataType::Date, DataType::Date) => DataType::Int,
                    (DataType::Interval, DataType::Interval) => DataType::Interval,
                    _ => numeric_result(l, r),
                },
                _ => numeric_result(l, r),
            }
        }
        Expr::Neg(e) => expr_type(e, schema, outer)?,
        Expr::Not(_) | Expr::IsNull { .. } | Expr::Like { .. } | Expr::InList { .. } => DataType::Bool,
        Expr::Exists { .. } | Expr::InSubquery { .. } => DataType::Bool,
        Expr::Case { whens, else_, .. } => {
            let mut t = DataType::Null;
            for (_, r) in whens {
                t = unify(t, expr_type(r, schema, outer)?);
            }
            if let Some(e) = else_ {
                t = unify(t, expr_type(e, schema, outer)?);
            }
            t
        }
        Expr::Cast { dtype, .. } => *dtype,
        Expr::Func { name, args } => {
            let arg_t = |i: usize| -> Result<DataType, String> {
                args.get(i).map(|a| expr_type(a, schema, outer)).unwrap_or(Ok(DataType::Null))
            };
            match name.as_str() {
                "year" | "month" | "day" | "quarter" | "dayofweek" | "dayofyear" | "week" | "length" | "strlen"
                | "char_length" | "sign" | "instr" | "strpos" => DataType::Int,
                "upper" | "lower" | "substring" | "trim" | "ltrim" | "rtrim" | "concat" | "replace" | "lpad"
                | "rpad" | "reverse" | "left" | "right" | "repeat" => DataType::Str,
                "abs" | "ceil" | "ceiling" | "floor" => arg_t(0)?,
                "round" => {
                    let t = arg_t(0)?;
                    if t == DataType::Int { DataType::Int } else { DataType::Float }
                }
                "sqrt" | "ln" | "log" | "log10" | "exp" | "power" | "pow" | "random" => DataType::Float,
                "coalesce" | "ifnull" | "greatest" | "least" | "nullif" => {
                    let mut t = DataType::Null;
                    for i in 0..args.len() {
                        t = unify(t, arg_t(i)?);
                    }
                    t
                }
                "date_trunc" => DataType::Date,
                "starts_with" | "contains" | "ends_with" => DataType::Bool,
                other => return Err(format!("Catalog error: unknown function {}", other)),
            }
        }
        Expr::Agg { func, arg, .. } => match func {
            AggFunc::Count | AggFunc::CountStar => DataType::Int,
            AggFunc::Avg => DataType::Float,
            AggFunc::Sum | AggFunc::Min | AggFunc::Max => {
                let t = expr_type(arg.as_ref().unwrap(), schema, outer)?;
                if t == DataType::Null && *func == AggFunc::Sum { DataType::Int } else { t }
            }
        },
        Expr::ScalarSubquery(p) => p.schema().fields.first().map(|f| f.dtype).unwrap_or(DataType::Null),
    })
}

pub fn unify(a: DataType, b: DataType) -> DataType {
    match (a, b) {
        (DataType::Null, x) | (x, DataType::Null) => x,
        (x, y) if x == y => x,
        (DataType::Int, DataType::Float) | (DataType::Float, DataType::Int) => DataType::Float,
        (_, DataType::Str) | (DataType::Str, _) => DataType::Str,
        (x, _) => x,
    }
}

pub fn lit_bool(b: bool) -> Expr {
    Expr::Lit(Value::Bool(b))
}
