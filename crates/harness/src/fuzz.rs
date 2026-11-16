//! Differential SQL fuzzer.
//!
//! Generates random, well-typed queries over two small tables with NULLs,
//! runs them in Quackless and DuckDB, and shrinks every mismatch to a minimal
//! query before saving it under tests/fuzz/ as a regression test.

use crate::common::*;
use crate::compare::compare;
use crate::oracle::Oracle;
use quackless_core::{Database, Options};
use std::fmt::Write as _;

// ---------------------------------------------------------------------- rng

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

// ---------------------------------------------------------------------- tables

const WORDS: &[&str] = &["apple", "Apple", "banana", "cherry", "date", "fig", "grape", "kiwi", ""];

/// CREATE + INSERT statements for t1(a, b, c, d) and t2(a, e, f).
pub fn gen_tables(rng: &mut Rng) -> Vec<String> {
    let mut out = vec![
        "CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE)".to_string(),
        "CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR)".to_string(),
    ];
    let int = |rng: &mut Rng| if rng.chance(0.15) { "NULL".to_string() } else { (rng.below(26) as i64 - 5).to_string() };
    let word = |rng: &mut Rng| if rng.chance(0.15) { "NULL".to_string() } else { format!("'{}'", rng.pick(WORDS)) };
    let n1 = 5 + rng.below(30);
    let rows: Vec<String> = (0..n1)
        .map(|_| {
            let d = if rng.chance(0.15) { "NULL".to_string() } else { format!("{}.{:02}", rng.below(200) as i64 - 50, rng.below(100)) };
            format!("({}, {}, {}, {})", int(rng), int(rng), word(rng), d)
        })
        .collect();
    out.push(format!("INSERT INTO t1 VALUES {}", rows.join(", ")));
    let n2 = 3 + rng.below(20);
    let rows: Vec<String> = (0..n2).map(|_| format!("({}, {}, {})", int(rng), int(rng), word(rng))).collect();
    out.push(format!("INSERT INTO t2 VALUES {}", rows.join(", ")));
    out
}

// ---------------------------------------------------------------------- expressions

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Str,
    Bool,
}

/// Expression tree kept as a tree so the shrinker can replace nodes.
#[derive(Debug, Clone)]
pub enum E {
    Leaf(String, Ty),
    /// template with `{}` holes, children, result type
    Node(&'static str, Vec<E>, Ty),
}

impl E {
    pub fn ty(&self) -> Ty {
        match self {
            E::Leaf(_, t) | E::Node(_, _, t) => *t,
        }
    }
    pub fn sql(&self) -> String {
        match self {
            E::Leaf(s, _) => s.clone(),
            E::Node(tpl, kids, _) => {
                let mut out = String::new();
                let mut k = kids.iter();
                let mut parts = tpl.split("{}");
                out.push_str(parts.next().unwrap_or(""));
                for p in parts {
                    out.push_str(&k.next().map(|e| e.sql()).unwrap_or_default());
                    out.push_str(p);
                }
                out
            }
        }
    }
    fn size(&self) -> usize {
        match self {
            E::Leaf(..) => 1,
            E::Node(_, k, _) => 1 + k.iter().map(|e| e.size()).sum::<usize>(),
        }
    }
}

pub struct Scope {
    pub cols: Vec<(String, Ty)>,
    /// false inside aggregate arguments: Quackless does not support subqueries there
    pub subqueries: bool,
}

fn leaf(rng: &mut Rng, sc: &Scope, ty: Ty) -> E {
    let cols: Vec<&(String, Ty)> = sc.cols.iter().filter(|c| c.1 == ty).collect();
    if !cols.is_empty() && rng.chance(0.7) {
        let c = cols[rng.below(cols.len())];
        return E::Leaf(c.0.clone(), ty);
    }
    let s = match ty {
        Ty::Int => {
            if rng.chance(0.05) { "CAST(NULL AS INTEGER)".to_string() } else { (rng.below(21) as i64 - 5).to_string() }
        }
        Ty::Float => format!("{}.5", rng.below(20) as i64 - 5),
        Ty::Str => format!("'{}'", rng.pick(WORDS)),
        Ty::Bool => (if rng.chance(0.5) { "TRUE" } else { "FALSE" }).to_string(),
    };
    E::Leaf(s, ty)
}

pub fn expr(rng: &mut Rng, sc: &Scope, ty: Ty, depth: usize) -> E {
    if depth == 0 || rng.chance(0.3) {
        if ty == Ty::Bool {
            // a bare boolean leaf is boring; compare two ints instead
            return E::Node("({} < {})", vec![leaf(rng, sc, Ty::Int), leaf(rng, sc, Ty::Int)], Ty::Bool);
        }
        return leaf(rng, sc, ty);
    }
    let d = depth - 1;
    let i = |rng: &mut Rng| expr(rng, sc, Ty::Int, d);
    match ty {
        Ty::Int => match rng.below(11) {
            // nullif keeps the divisor nonzero: whether DuckDB raises "division by zero"
            // depends on which rows its plan evaluates, which is not worth matching
            8 => E::Node("({} % nullif({}, 0))", vec![i(rng), i(rng)], Ty::Int),
            9 => E::Node("nullif({}, {})", vec![i(rng), i(rng)], Ty::Int),
            10 if sc.subqueries => E::Node("(SELECT max(e) FROM t2 WHERE t2.e < {})", vec![leaf(rng, sc, Ty::Int)], Ty::Int),
            10 => E::Node("({} + 1)", vec![i(rng)], Ty::Int),
            0 => E::Node("({} + {})", vec![i(rng), i(rng)], Ty::Int),
            1 => E::Node("({} - {})", vec![i(rng), i(rng)], Ty::Int),
            2 => E::Node("({} * {})", vec![i(rng), i(rng)], Ty::Int),
            3 => E::Node("abs({})", vec![i(rng)], Ty::Int),
            4 => E::Node("coalesce({}, {})", vec![i(rng), i(rng)], Ty::Int),
            5 => E::Node("CASE WHEN {} THEN {} ELSE {} END", vec![expr(rng, sc, Ty::Bool, d), i(rng), i(rng)], Ty::Int),
            6 => E::Node("length({})", vec![expr(rng, sc, Ty::Str, d)], Ty::Int),
            _ => E::Node("(- {})", vec![i(rng)], Ty::Int),
        },
        Ty::Float => match rng.below(5) {
            4 => E::Node("({} / nullif({}, 0))", vec![i(rng), i(rng)], Ty::Float),
            0 => E::Node("({} + {})", vec![expr(rng, sc, Ty::Float, d), i(rng)], Ty::Float),
            1 => E::Node("({} * 2.0)", vec![expr(rng, sc, Ty::Float, d)], Ty::Float),
            2 => E::Node("({} / 4.0)", vec![i(rng)], Ty::Float),
            _ => E::Node("coalesce({}, 0.25)", vec![expr(rng, sc, Ty::Float, d)], Ty::Float),
        },
        Ty::Str => match rng.below(7) {
            5 => E::Node("CAST({} AS VARCHAR)", vec![i(rng)], Ty::Str),
            6 => E::Node("CASE WHEN {} THEN {} END", vec![expr(rng, sc, Ty::Bool, d), expr(rng, sc, Ty::Str, d)], Ty::Str),
            0 => E::Node("upper({})", vec![expr(rng, sc, Ty::Str, d)], Ty::Str),
            1 => E::Node("lower({})", vec![expr(rng, sc, Ty::Str, d)], Ty::Str),
            2 => E::Node("({} || {})", vec![expr(rng, sc, Ty::Str, d), expr(rng, sc, Ty::Str, d)], Ty::Str),
            3 => E::Node("substring({}, 2, 3)", vec![expr(rng, sc, Ty::Str, d)], Ty::Str),
            _ => E::Node("CASE WHEN {} THEN {} ELSE {} END", vec![expr(rng, sc, Ty::Bool, d), expr(rng, sc, Ty::Str, d), expr(rng, sc, Ty::Str, d)], Ty::Str),
        },
        Ty::Bool => match rng.below(15) {
            11 if sc.subqueries => E::Node("({} IN (SELECT e FROM t2))", vec![i(rng)], Ty::Bool),
            12 if sc.subqueries => E::Node("({} NOT IN (SELECT a FROM t2 WHERE a IS NOT NULL))", vec![i(rng)], Ty::Bool),
            13 if sc.subqueries => E::Node("EXISTS (SELECT 1 FROM t2 WHERE t2.e = {})", vec![leaf(rng, sc, Ty::Int)], Ty::Bool),
            11..=13 => E::Node("({} > {})", vec![i(rng), i(rng)], Ty::Bool),
            14 => E::Node("({} IS NOT DISTINCT FROM {})", vec![i(rng), i(rng)], Ty::Bool),
            0 => E::Node("({} = {})", vec![i(rng), i(rng)], Ty::Bool),
            1 => E::Node("({} <> {})", vec![i(rng), i(rng)], Ty::Bool),
            2 => E::Node("({} >= {})", vec![i(rng), i(rng)], Ty::Bool),
            3 => E::Node("({} AND {})", vec![expr(rng, sc, Ty::Bool, d), expr(rng, sc, Ty::Bool, d)], Ty::Bool),
            4 => E::Node("({} OR {})", vec![expr(rng, sc, Ty::Bool, d), expr(rng, sc, Ty::Bool, d)], Ty::Bool),
            5 => E::Node("(NOT {})", vec![expr(rng, sc, Ty::Bool, d)], Ty::Bool),
            6 => E::Node("({} IS NULL)", vec![i(rng)], Ty::Bool),
            7 => E::Node("({} BETWEEN {} AND {})", vec![i(rng), i(rng), i(rng)], Ty::Bool),
            8 => E::Node("({} IN ({}, {}, {}))", vec![i(rng), i(rng), i(rng), i(rng)], Ty::Bool),
            9 => {
                let pats = ["'a%'", "'%e'", "'%an%'", "'_i%'", "'A%'"];
                let p = rng.pick(&pats).to_string();
                E::Node("({} LIKE {})", vec![expr(rng, sc, Ty::Str, d), E::Leaf(p, Ty::Str)], Ty::Bool)
            }
            _ => E::Node("({} < {})", vec![expr(rng, sc, Ty::Str, d), expr(rng, sc, Ty::Str, d)], Ty::Bool),
        },
    }
}

// ---------------------------------------------------------------------- queries

#[derive(Debug, Clone)]
pub struct Item {
    pub e: E,
    pub agg: Option<&'static str>,
}

impl Item {
    fn sql(&self) -> String {
        match self.agg {
            None => self.e.sql(),
            Some("count(*)") => "count(*)".into(),
            Some("count(DISTINCT") => format!("count(DISTINCT {})", self.e.sql()),
            Some("sum+count") => format!("(sum({}) + count(*))", self.e.sql()),
            Some(f) => format!("{}({})", f, self.e.sql()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Q {
    /// UNION ALL / UNION / EXCEPT / INTERSECT with a second simple query
    pub setop: Option<(&'static str, Box<Q>)>,
    pub desc: bool,
    pub from: String,
    pub select: Vec<Item>,
    pub filter: Option<E>,
    pub group: Vec<E>,
    pub having: Option<E>,
    pub order: bool,
    pub limit: Option<usize>,
    pub distinct: bool,
}

impl Q {
    pub fn sql(&self) -> String {
        let mut s = String::from("SELECT ");
        if self.distinct {
            s.push_str("DISTINCT ");
        }
        s.push_str(&self.select.iter().enumerate().map(|(i, it)| format!("{} AS c{}", it.sql(), i)).collect::<Vec<_>>().join(", "));
        let _ = write!(s, " FROM {}", self.from);
        if let Some(f) = &self.filter {
            let _ = write!(s, " WHERE {}", f.sql());
        }
        if !self.group.is_empty() {
            let _ = write!(s, " GROUP BY {}", self.group.iter().map(|e| e.sql()).collect::<Vec<_>>().join(", "));
        }
        if let Some(h) = &self.having {
            let _ = write!(s, " HAVING {}", h.sql());
        }
        if let Some((op, other)) = &self.setop {
            let _ = write!(s, " {} {}", op, other.sql());
        }
        if self.order || self.limit.is_some() {
            // order by every output column so LIMIT is deterministic
            let dir = if self.desc { " DESC" } else { "" };
            let _ = write!(s, " ORDER BY {}", (1..=self.select.len()).map(|i| format!("{}{}", i, dir)).collect::<Vec<_>>().join(", "));
        }
        if let Some(l) = self.limit {
            let _ = write!(s, " LIMIT {}", l);
        }
        s
    }
}

const FROMS: &[(&str, &[(&str, Ty)])] = &[
    ("t1", &[("a", Ty::Int), ("b", Ty::Int), ("c", Ty::Str), ("d", Ty::Float)]),
    ("t2", &[("a", Ty::Int), ("e", Ty::Int), ("f", Ty::Str)]),
    ("t1 JOIN t2 ON t1.a = t2.a", &[("t1.a", Ty::Int), ("b", Ty::Int), ("c", Ty::Str), ("d", Ty::Float), ("e", Ty::Int), ("f", Ty::Str)]),
    ("t1 LEFT JOIN t2 ON t1.a = t2.a", &[("t1.a", Ty::Int), ("b", Ty::Int), ("c", Ty::Str), ("d", Ty::Float), ("t2.a", Ty::Int), ("e", Ty::Int), ("f", Ty::Str)]),
    ("t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3", &[("t1.a", Ty::Int), ("b", Ty::Int), ("c", Ty::Str), ("e", Ty::Int), ("f", Ty::Str)]),
    ("(SELECT a, b + 1 AS b2, c FROM t1 WHERE b IS NOT NULL) AS s", &[("a", Ty::Int), ("b2", Ty::Int), ("c", Ty::Str)]),
    ("t1, t2", &[("t1.a", Ty::Int), ("b", Ty::Int), ("e", Ty::Int), ("f", Ty::Str)]),
];

/// A GROUP BY key that always references a column (a constant key is read as a
/// column position by DuckDB, as an expression by Quackless).
fn group_key(rng: &mut Rng, sc: &Scope) -> E {
    let cols: Vec<&(String, Ty)> = sc.cols.iter().filter(|c| c.1 == Ty::Int || c.1 == Ty::Str).collect();
    let (name, ty) = cols[rng.below(cols.len())].clone();
    let col = E::Leaf(name, ty);
    match (ty, rng.below(4)) {
        (Ty::Int, 0) => E::Node("({} % 3)", vec![col], Ty::Int),
        (Ty::Int, 1) => E::Node("coalesce({}, 0)", vec![col], Ty::Int),
        (Ty::Str, 0) => E::Node("upper({})", vec![col], Ty::Str),
        (Ty::Str, 1) => E::Node("length({})", vec![col], Ty::Int),
        _ => col,
    }
}

pub fn gen_query(rng: &mut Rng) -> Q {
    let (from, cols) = FROMS[rng.below(FROMS.len())];
    let sc = Scope { cols: cols.iter().map(|(n, t)| (n.to_string(), *t)).collect(), subqueries: true };
    let agg_sc = Scope { cols: sc.cols.clone(), subqueries: false };
    let filter = if rng.chance(0.6) { Some(expr(rng, &sc, Ty::Bool, 3)) } else { None };
    let tys = [Ty::Int, Ty::Int, Ty::Str, Ty::Float, Ty::Bool];
    let mut q = Q { setop: None, desc: false, from: from.to_string(), select: vec![], filter, group: vec![], having: None, order: false, limit: None, distinct: false };
    if rng.chance(0.45) {
        // grouped query: group keys are selected, plus aggregates
        let ng = 1 + rng.below(2);
        for _ in 0..ng {
            let g = group_key(rng, &sc);
            q.select.push(Item { e: g.clone(), agg: None });
            q.group.push(g);
        }
        for _ in 0..1 + rng.below(3) {
            let f = *rng.pick(&["sum", "count", "min", "max", "avg", "count(*)", "count(DISTINCT", "sum+count"]);
            let t = if f == "min" || f == "max" { *rng.pick(&[Ty::Int, Ty::Str, Ty::Float]) } else { Ty::Int };
            let e = expr(rng, &agg_sc, t, 2);
            q.select.push(Item { e, agg: Some(f) });
        }
        if rng.chance(0.4) {
            let a = expr(rng, &agg_sc, Ty::Int, 1);
            q.having = Some(E::Node(match rng.below(4) { 0 => "count({}) > 1", 1 => "sum({}) < 3", 2 => "max({}) >= 0", _ => "min({}) IS NOT NULL" }, vec![a], Ty::Bool));
        }
    } else {
        for _ in 0..1 + rng.below(4) {
            let t = *rng.pick(&tys);
            q.select.push(Item { e: expr(rng, &sc, t, 3), agg: None });
        }
        q.distinct = rng.chance(0.15);
        if rng.chance(0.15) {
            // a second query with the same column types over t2
            let sc2 = Scope { cols: vec![("a".into(), Ty::Int), ("e".into(), Ty::Int), ("f".into(), Ty::Str)], subqueries: true };
            let select = q.select.iter().map(|it| Item { e: expr(rng, &sc2, it.e.ty(), 1), agg: None }).collect();
            let filter = if rng.chance(0.5) { Some(expr(rng, &sc2, Ty::Bool, 2)) } else { None };
            let other = Q { setop: None, desc: false, from: "t2".into(), select, filter, group: vec![], having: None, order: false, limit: None, distinct: false };
            let op = *rng.pick(&["UNION ALL", "UNION", "EXCEPT", "INTERSECT"]);
            q.setop = Some((op, Box::new(other)));
        }
    }
    q.order = rng.chance(0.4);
    q.desc = rng.chance(0.3);
    if rng.chance(0.3) {
        q.limit = Some(rng.below(8));
    }
    q
}

// ---------------------------------------------------------------------- running

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Agree,
    BothError,
    /// DuckDB rejected the query (outside the shared dialect): not counted.
    Invalid,
    Mismatch(String),
}

pub fn check(db: &Database, oracle: &mut Oracle, sql: &str) -> Verdict {
    let duck = oracle.query(sql);
    let ours = db.query(sql, Options::default());
    match (ours, duck) {
        (Err(_), Err(_)) => Verdict::BothError,
        (Ok(_), Err(_)) => Verdict::Invalid,
        (Err(e), Ok(_)) => Verdict::Mismatch(format!("Quackless error: {}", e)),
        (Ok(r), Ok(d)) => match compare(&ours_cells(&r), &duck_cells(&d.rows), is_ordered(sql)) {
            Ok(()) => Verdict::Agree,
            Err(e) => Verdict::Mismatch(e),
        },
    }
}

// ---------------------------------------------------------------------- shrinking

/// Every expression one step simpler than `e`: each child of a node (if same type), or a literal.
fn simpler(e: &E) -> Vec<E> {
    let mut out = Vec::new();
    if let E::Node(tpl, kids, ty) = e {
        for k in kids {
            if k.ty() == *ty {
                out.push(k.clone());
            }
        }
        for (i, k) in kids.iter().enumerate() {
            for s in simpler(k) {
                let mut kk = kids.clone();
                kk[i] = s;
                out.push(E::Node(tpl, kk, *ty));
            }
        }
        let lit = match ty {
            Ty::Int => "1",
            Ty::Float => "1.5",
            Ty::Str => "'apple'",
            Ty::Bool => "TRUE",
        };
        out.push(E::Leaf(lit.into(), *ty));
    }
    out
}

fn candidates(q: &Q) -> Vec<Q> {
    let mut v = Vec::new();
    let mut push = |c: Q| v.push(c);
    if q.limit.is_some() {
        push(Q { limit: None, ..q.clone() });
    }
    if q.setop.is_some() {
        push(Q { setop: None, ..q.clone() });
    }
    if q.order {
        push(Q { order: false, ..q.clone() });
    }
    if q.distinct {
        push(Q { distinct: false, ..q.clone() });
    }
    if q.having.is_some() {
        push(Q { having: None, ..q.clone() });
    }
    if q.filter.is_some() {
        push(Q { filter: None, ..q.clone() });
    }
    if q.select.len() > 1 {
        for i in 0..q.select.len() {
            let mut c = q.clone();
            let it = c.select.remove(i);
            // a group key must stay selected only if it is still grouped; drop both together
            if it.agg.is_none() && !c.group.is_empty() {
                if let Some(p) = c.group.iter().position(|g| g.sql() == it.e.sql()) {
                    c.group.remove(p);
                }
            }
            if c.select.iter().all(|s| s.agg.is_none()) && !c.group.is_empty() {
                continue;
            }
            push(c);
        }
    }
    if let Some(f) = &q.filter {
        for s in simpler(f) {
            push(Q { filter: Some(s), ..q.clone() });
        }
    }
    for (i, it) in q.select.iter().enumerate() {
        if it.agg.is_none() && !q.group.is_empty() {
            continue; // keep group keys and selected keys in step
        }
        for s in simpler(&it.e) {
            let mut c = q.clone();
            c.select[i].e = s;
            push(c);
        }
    }
    v
}

/// Greedy shrink: keep taking the first simpler query that still mismatches.
pub fn shrink(db: &Database, oracle: &mut Oracle, q: Q) -> Q {
    let mut cur = q;
    let mut budget = 400;
    'outer: while budget > 0 {
        for c in candidates(&cur) {
            budget -= 1;
            if matches!(check(db, oracle, &c.sql()), Verdict::Mismatch(_)) {
                cur = c;
                continue 'outer;
            }
            if budget == 0 {
                break;
            }
        }
        break;
    }
    cur
}

pub fn query_size(q: &Q) -> usize {
    q.select.iter().map(|i| i.e.size()).sum::<usize>() + q.filter.as_ref().map(|f| f.size()).unwrap_or(0) + q.group.len()
}

// ---------------------------------------------------------------------- driver

pub struct FuzzStats {
    pub queries: usize,
    pub agree: usize,
    pub both_error: usize,
    pub invalid: usize,
    pub mismatches: Vec<(String, String, Vec<String>)>,
}

pub fn run(seed: u64, n: usize) -> Result<FuzzStats, String> {
    let root = root();
    let mut oracle = Oracle::spawn(&root)?;
    let mut rng = Rng::new(seed);
    let mut st = FuzzStats { queries: 0, agree: 0, both_error: 0, invalid: 0, mismatches: vec![] };
    let mut db = Database::new();
    let mut setup = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for k in 0..n {
        if k % 250 == 0 {
            oracle.reset()?;
            db = Database::new();
            setup = gen_tables(&mut rng);
            for s in &setup {
                db.execute(s, Options::default())?;
                oracle.query(s)?;
            }
        }
        let q = gen_query(&mut rng);
        let sql = q.sql();
        st.queries += 1;
        match check(&db, &mut oracle, &sql) {
            Verdict::Agree => st.agree += 1,
            Verdict::BothError => {
                st.both_error += 1;
                if std::env::var("FUZZ_SHOW").is_ok() {
                    eprintln!("both error: {}  ({:?})", sql, db.query(&sql, Options::default()).err());
                }
            }
            Verdict::Invalid => {
                st.invalid += 1;
                if std::env::var("FUZZ_SHOW").is_ok() {
                    eprintln!("rejected by DuckDB: {}  ({:?})", sql, oracle.query(&sql).err());
                }
            }
            Verdict::Mismatch(_) => {
                let small = shrink(&db, &mut oracle, q);
                let ssql = small.sql();
                let why = match check(&db, &mut oracle, &ssql) {
                    Verdict::Mismatch(m) => m,
                    _ => "not reproducible after shrinking".into(),
                };
                if seen.insert(ssql.clone()) {
                    eprintln!("mismatch (size {}): {}\n   {}", query_size(&small), ssql, why);
                    st.mismatches.push((ssql, why, setup.clone()));
                }
            }
        }
    }
    Ok(st)
}

/// Save a minimized mismatch as tests/fuzz/<name>.sql with DuckDB's answer, so
/// `cargo test` can check it without DuckDB.
pub fn save_regression(oracle: &mut Oracle, name: &str, setup: &[String], sql: &str) -> Result<std::path::PathBuf, String> {
    oracle.reset()?;
    for s in setup {
        oracle.query(s)?;
    }
    let d = oracle.query(sql)?;
    let dir = root().join("tests/fuzz/open");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut text = String::from("-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.\n");
    for s in setup {
        let _ = writeln!(text, "{};", s);
    }
    let _ = writeln!(text, "-- query\n{};", sql);
    let _ = writeln!(text, "-- expected\n{}", serde_json::Value::Array(d.rows.into_iter().map(serde_json::Value::Array).collect()));
    let p = dir.join(format!("{}.sql", name));
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Parse a saved regression file into (setup statements, query, expected rows).
pub fn load_regression(text: &str) -> (Vec<String>, String, Vec<Vec<serde_json::Value>>) {
    let (head, rest) = text.split_once("-- query\n").unwrap_or((text, ""));
    let (q, exp) = rest.split_once("-- expected\n").unwrap_or((rest, "[]"));
    let setup = head.lines().filter(|l| !l.starts_with("--") && !l.trim().is_empty()).map(|l| l.trim().trim_end_matches(';').to_string()).collect();
    let rows: Vec<Vec<serde_json::Value>> = serde_json::from_str::<serde_json::Value>(exp.trim())
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.as_array().cloned().unwrap_or_default())
        .collect();
    (setup, q.trim().trim_end_matches(';').to_string(), rows)
}

/// Move every saved mismatch in tests/fuzz/open/ that Quackless now answers like
/// DuckDB into tests/fuzz/, where `cargo test` asserts it. Returns (fixed, still open).
pub fn triage() -> Result<(usize, usize), String> {
    let open = root().join("tests/fuzz/open");
    let done = root().join("tests/fuzz");
    let mut fixed = 0;
    let mut still = 0;
    let Ok(rd) = std::fs::read_dir(&open) else { return Ok((0, 0)) };
    let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false)).collect();
    files.sort();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
        let (setup, sql, rows) = load_regression(&text);
        let mut db = Database::new();
        let ok = setup.iter().all(|s| db.execute(s, Options::default()).is_ok())
            && db.query(&sql, Options::default()).map(|r| compare(&ours_cells(&r), &duck_cells(&rows), is_ordered(&sql)).is_ok()).unwrap_or(false);
        if ok {
            std::fs::rename(&f, done.join(f.file_name().unwrap())).map_err(|e| e.to_string())?;
            fixed += 1;
        } else {
            still += 1;
        }
    }
    Ok((fixed, still))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_is_deterministic_and_parses() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut db = Database::new();
        for s in gen_tables(&mut Rng::new(1)) {
            db.execute(&s, Options::default()).unwrap();
        }
        for _ in 0..300 {
            let (qa, qb) = (gen_query(&mut a).sql(), gen_query(&mut b).sql());
            assert_eq!(qa, qb);
            quackless_core::parser::parse_statement(&qa).unwrap_or_else(|e| panic!("{}: {}", qa, e));
        }
    }

    #[test]
    fn shrink_candidates_are_smaller() {
        let mut r = Rng::new(3);
        for _ in 0..50 {
            let q = gen_query(&mut r);
            for c in candidates(&q) {
                assert!(query_size(&c) <= query_size(&q));
            }
        }
    }

    /// Every saved fuzzer regression still matches DuckDB's recorded answer.
    #[test]
    fn saved_regressions_pass() {
        let dir = root().join("tests/fuzz");
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for f in rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "sql").unwrap_or(false)) {
            let text = std::fs::read_to_string(&f).unwrap();
            let (setup, sql, rows) = load_regression(&text);
            let mut db = Database::new();
            for s in &setup {
                db.execute(s, Options::default()).unwrap();
            }
            let r = db.query(&sql, Options::default()).unwrap_or_else(|e| panic!("{}: {}", f.display(), e));
            compare(&ours_cells(&r), &duck_cells(&rows), is_ordered(&sql)).unwrap_or_else(|e| panic!("{}: {}", f.display(), e));
        }
    }
}
