//! Bound (resolved) expressions used by logical and physical plans.

use crate::ast::BinOp;
use crate::plan::LogicalPlan;
use crate::types::{DataType, Value};
use std::collections::BTreeSet;
use std::fmt;

/// A fully resolved column reference: relation qualifier (table alias,
/// derived-table alias or None for computed columns) plus column name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColRef {
    pub rel: Option<String>,
    pub name: String,
}

impl ColRef {
    pub fn new(rel: Option<&str>, name: &str) -> Self {
        ColRef { rel: rel.map(|s| s.to_string()), name: name.to_string() }
    }
}

impl fmt::Display for ColRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.rel {
            Some(r) if !r.starts_with("__") => write!(f, "{}.{}", r, self.name),
            _ => write!(f, "{}", self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggFunc {
    Count,
    CountStar,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggFunc {
    pub fn name(self) -> &'static str {
        match self {
            AggFunc::Count | AggFunc::CountStar => "count",
            AggFunc::Sum => "sum",
            AggFunc::Avg => "avg",
            AggFunc::Min => "min",
            AggFunc::Max => "max",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Column(ColRef),
    /// Correlated reference to a column of the enclosing query.
    Outer(ColRef),
    Lit(Value),
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Neg(Box<Expr>),
    Not(Box<Expr>),
    IsNull { expr: Box<Expr>, negated: bool },
    Like { expr: Box<Expr>, pattern: Box<Expr>, negated: bool },
    InList { expr: Box<Expr>, list: Vec<Expr>, negated: bool },
    Case { operand: Option<Box<Expr>>, whens: Vec<(Expr, Expr)>, else_: Option<Box<Expr>> },
    Cast { expr: Box<Expr>, dtype: DataType },
    Func { name: String, args: Vec<Expr> },
    Agg { func: AggFunc, arg: Option<Box<Expr>>, distinct: bool },
    ScalarSubquery(Box<LogicalPlan>),
    Exists { plan: Box<LogicalPlan>, negated: bool },
    InSubquery { expr: Box<Expr>, plan: Box<LogicalPlan>, negated: bool },
}

impl Expr {
    pub fn col(rel: Option<&str>, name: &str) -> Expr {
        Expr::Column(ColRef::new(rel, name))
    }
    pub fn lit(v: Value) -> Expr {
        Expr::Lit(v)
    }
    pub fn binary(op: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::Binary { op, left: Box::new(l), right: Box::new(r) }
    }

    /// Immediate children (does not descend into subquery plans).
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Column(_) | Expr::Outer(_) | Expr::Lit(_) | Expr::ScalarSubquery(_) | Expr::Exists { .. } => vec![],
            Expr::Binary { left, right, .. } => vec![left, right],
            Expr::Neg(e) | Expr::Not(e) | Expr::IsNull { expr: e, .. } | Expr::Cast { expr: e, .. } => vec![e],
            Expr::Like { expr, pattern, .. } => vec![expr, pattern],
            Expr::InList { expr, list, .. } => {
                let mut v = vec![expr.as_ref()];
                v.extend(list.iter());
                v
            }
            Expr::Case { operand, whens, else_ } => {
                let mut v = Vec::new();
                if let Some(o) = operand {
                    v.push(o.as_ref());
                }
                for (a, b) in whens {
                    v.push(a);
                    v.push(b);
                }
                if let Some(e) = else_ {
                    v.push(e.as_ref());
                }
                v
            }
            Expr::Func { args, .. } => args.iter().collect(),
            Expr::Agg { arg, .. } => arg.iter().map(|a| a.as_ref()).collect(),
            Expr::InSubquery { expr, .. } => vec![expr],
        }
    }

    /// Bottom-up rewrite. `f` is applied after the children were rewritten.
    pub fn transform(self, f: &mut dyn FnMut(Expr) -> Result<Expr, String>) -> Result<Expr, String> {
        let e = match self {
            Expr::Binary { op, left, right } => {
                Expr::Binary { op, left: Box::new(left.transform(f)?), right: Box::new(right.transform(f)?) }
            }
            Expr::Neg(e) => Expr::Neg(Box::new(e.transform(f)?)),
            Expr::Not(e) => Expr::Not(Box::new(e.transform(f)?)),
            Expr::IsNull { expr, negated } => Expr::IsNull { expr: Box::new(expr.transform(f)?), negated },
            Expr::Cast { expr, dtype } => Expr::Cast { expr: Box::new(expr.transform(f)?), dtype },
            Expr::Like { expr, pattern, negated } => {
                Expr::Like { expr: Box::new(expr.transform(f)?), pattern: Box::new(pattern.transform(f)?), negated }
            }
            Expr::InList { expr, list, negated } => Expr::InList {
                expr: Box::new(expr.transform(f)?),
                list: list.into_iter().map(|e| e.transform(f)).collect::<Result<_, _>>()?,
                negated,
            },
            Expr::Case { operand, whens, else_ } => Expr::Case {
                operand: match operand {
                    Some(o) => Some(Box::new(o.transform(f)?)),
                    None => None,
                },
                whens: whens
                    .into_iter()
                    .map(|(a, b)| Ok((a.transform(f)?, b.transform(f)?)))
                    .collect::<Result<_, String>>()?,
                else_: match else_ {
                    Some(e) => Some(Box::new(e.transform(f)?)),
                    None => None,
                },
            },
            Expr::Func { name, args } => {
                Expr::Func { name, args: args.into_iter().map(|e| e.transform(f)).collect::<Result<_, _>>()? }
            }
            Expr::Agg { func, arg, distinct } => Expr::Agg {
                func,
                arg: match arg {
                    Some(a) => Some(Box::new(a.transform(f)?)),
                    None => None,
                },
                distinct,
            },
            Expr::InSubquery { expr, plan, negated } => {
                Expr::InSubquery { expr: Box::new(expr.transform(f)?), plan, negated }
            }
            other => other,
        };
        f(e)
    }

    /// Top-down rewrite: if `f` returns Some, the node is replaced and not descended.
    pub fn replace(self, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> Expr {
        if let Some(r) = f(&self) {
            return r;
        }
        let g = |e: Expr| -> Result<Expr, String> { Ok(e) };
        // rebuild children with replacement
        match self {
            Expr::Binary { op, left, right } => {
                Expr::Binary { op, left: Box::new(left.replace(f)), right: Box::new(right.replace(f)) }
            }
            Expr::Neg(e) => Expr::Neg(Box::new(e.replace(f))),
            Expr::Not(e) => Expr::Not(Box::new(e.replace(f))),
            Expr::IsNull { expr, negated } => Expr::IsNull { expr: Box::new(expr.replace(f)), negated },
            Expr::Cast { expr, dtype } => Expr::Cast { expr: Box::new(expr.replace(f)), dtype },
            Expr::Like { expr, pattern, negated } => {
                Expr::Like { expr: Box::new(expr.replace(f)), pattern: Box::new(pattern.replace(f)), negated }
            }
            Expr::InList { expr, list, negated } => Expr::InList {
                expr: Box::new(expr.replace(f)),
                list: list.into_iter().map(|e| e.replace(f)).collect(),
                negated,
            },
            Expr::Case { operand, whens, else_ } => Expr::Case {
                operand: operand.map(|o| Box::new(o.replace(f))),
                whens: whens.into_iter().map(|(a, b)| (a.replace(f), b.replace(f))).collect(),
                else_: else_.map(|e| Box::new(e.replace(f))),
            },
            Expr::Func { name, args } => Expr::Func { name, args: args.into_iter().map(|e| e.replace(f)).collect() },
            Expr::Agg { func, arg, distinct } => {
                Expr::Agg { func, arg: arg.map(|a| Box::new(a.replace(f))), distinct }
            }
            Expr::InSubquery { expr, plan, negated } => {
                Expr::InSubquery { expr: Box::new(expr.replace(f)), plan, negated }
            }
            other => g(other).unwrap(),
        }
    }

    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        for c in self.children() {
            c.walk(f);
        }
    }

    /// Columns of the current scope referenced by this expression.
    pub fn columns(&self) -> BTreeSet<ColRef> {
        let mut out = BTreeSet::new();
        self.walk(&mut |e| match e {
            Expr::Column(c) => {
                out.insert(c.clone());
            }
            // a correlated subquery needs the outer columns it refers to
            Expr::ScalarSubquery(p) | Expr::Exists { plan: p, .. } | Expr::InSubquery { plan: p, .. } => {
                out.extend(p.outer_refs());
            }
            _ => {}
        });
        out
    }

    pub fn contains_agg(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| {
            if matches!(e, Expr::Agg { .. }) {
                found = true
            }
        });
        found
    }

    pub fn contains_subquery(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| {
            if matches!(e, Expr::ScalarSubquery(_) | Expr::Exists { .. } | Expr::InSubquery { .. }) {
                found = true
            }
        });
        found
    }

    pub fn contains_outer(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| match e {
            Expr::Outer(_) => found = true,
            Expr::ScalarSubquery(p) | Expr::Exists { plan: p, .. } | Expr::InSubquery { plan: p, .. } => {
                // a subquery that is correlated with *our* scope does not make us correlated
                let _ = p;
            }
            _ => {}
        });
        found
    }

    pub fn is_volatile(&self) -> bool {
        let mut v = false;
        self.walk(&mut |e| {
            if let Expr::Func { name, .. } = e {
                if name == "random" {
                    v = true
                }
            }
        });
        v
    }
}

pub fn split_conjuncts(e: Expr) -> Vec<Expr> {
    match e {
        Expr::Binary { op: BinOp::And, left, right } => {
            let mut v = split_conjuncts(*left);
            v.extend(split_conjuncts(*right));
            v
        }
        other => vec![other],
    }
}

pub fn split_disjuncts(e: &Expr) -> Vec<Expr> {
    match e {
        Expr::Binary { op: BinOp::Or, left, right } => {
            let mut v = split_disjuncts(left);
            v.extend(split_disjuncts(right));
            v
        }
        other => vec![other.clone()],
    }
}

pub fn conjunction(v: Vec<Expr>) -> Option<Expr> {
    v.into_iter().reduce(|a, b| Expr::binary(BinOp::And, a, b))
}

pub fn disjunction(v: Vec<Expr>) -> Option<Expr> {
    v.into_iter().reduce(|a, b| Expr::binary(BinOp::Or, a, b))
}

fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Binary { op, .. } => match op {
            BinOp::Or => 1,
            BinOp::And => 2,
            o if o.is_comparison() => 4,
            BinOp::Concat => 5,
            BinOp::Plus | BinOp::Minus => 6,
            _ => 7,
        },
        Expr::Not(_) => 3,
        Expr::Like { .. } | Expr::InList { .. } | Expr::IsNull { .. } | Expr::InSubquery { .. } => 4,
        _ => 9,
    }
}

fn paren(f: &mut fmt::Formatter<'_>, e: &Expr, min: u8) -> fmt::Result {
    if prec(e) < min {
        write!(f, "({})", e)
    } else {
        write!(f, "{}", e)
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Column(c) => write!(f, "{}", c),
            Expr::Outer(c) => write!(f, "outer.{}", c.name),
            Expr::Lit(v) => match v {
                Value::Str(s) => write!(f, "'{}'", s.replace('\'', "''")),
                Value::Date(_) => write!(f, "DATE '{}'", v),
                Value::Interval { .. } => write!(f, "INTERVAL '{}'", v),
                _ => write!(f, "{}", v),
            },
            Expr::Binary { op, left, right } => {
                let p = prec(self);
                paren(f, left, p)?;
                write!(f, " {} ", op.symbol())?;
                paren(f, right, p + 1)
            }
            Expr::Neg(e) => {
                write!(f, "-")?;
                paren(f, e, 9)
            }
            Expr::Not(e) => {
                write!(f, "NOT ")?;
                paren(f, e, 4)
            }
            Expr::IsNull { expr, negated } => {
                paren(f, expr, 5)?;
                write!(f, " IS {}NULL", if *negated { "NOT " } else { "" })
            }
            Expr::Like { expr, pattern, negated } => {
                paren(f, expr, 5)?;
                write!(f, " {}LIKE {}", if *negated { "NOT " } else { "" }, pattern)
            }
            Expr::InList { expr, list, negated } => {
                paren(f, expr, 5)?;
                write!(f, " {}IN (", if *negated { "NOT " } else { "" })?;
                for (i, e) in list.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, ")")
            }
            Expr::Case { operand, whens, else_ } => {
                write!(f, "CASE")?;
                if let Some(o) = operand {
                    write!(f, " {}", o)?;
                }
                for (a, b) in whens {
                    write!(f, " WHEN {} THEN {}", a, b)?;
                }
                if let Some(e) = else_ {
                    write!(f, " ELSE {}", e)?;
                }
                write!(f, " END")
            }
            Expr::Cast { expr, dtype } => write!(f, "CAST({} AS {})", expr, dtype.name()),
            Expr::Func { name, args } => {
                write!(f, "{}(", name)?;
                for (i, e) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, ")")
            }
            Expr::Agg { func, arg, distinct } => match (func, arg) {
                (AggFunc::CountStar, _) => write!(f, "count_star()"),
                (_, Some(a)) => write!(f, "{}({}{})", func.name(), if *distinct { "DISTINCT " } else { "" }, a),
                (_, None) => write!(f, "{}()", func.name()),
            },
            Expr::ScalarSubquery(_) => write!(f, "(SUBQUERY)"),
            Expr::Exists { negated, .. } => write!(f, "{}EXISTS(SUBQUERY)", if *negated { "NOT " } else { "" }),
            Expr::InSubquery { expr, negated, .. } => {
                paren(f, expr, 5)?;
                write!(f, " {}IN (SUBQUERY)", if *negated { "NOT " } else { "" })
            }
        }
    }
}
