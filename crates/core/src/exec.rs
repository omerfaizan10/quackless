//! Physical operators (pull-based, one batch of up to BATCH_SIZE rows per call)
//! and the physical planner that turns an optimized logical plan into them.

use crate::catalog::Catalog;
use crate::eval::{compile, encode_cell, eval, PExpr};
use crate::expr::{AggFunc, Expr};
use crate::plan::{expr_type, unify, JoinType, LogicalPlan, Schema};
use crate::types::*;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

pub const BATCH_SIZE: usize = 2048;

// ---------------------------------------------------------------------- clock

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    use std::time::Instant;
    thread_local! { static START: Instant = Instant::now(); }
    START.with(|s| s.elapsed().as_secs_f64() * 1000.0)
}

#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> f64 {
    use wasm_bindgen::JsCast;
    let g = js_sys::global();
    let perf = js_sys::Reflect::get(&g, &"performance".into()).ok();
    perf.and_then(|p| {
        let f = js_sys::Reflect::get(&p, &"now".into()).ok()?;
        let f: js_sys::Function = f.dyn_into().ok()?;
        f.call0(&p).ok()?.as_f64()
    })
    .unwrap_or_else(js_sys::Date::now)
}

// ---------------------------------------------------------------------- hashing

/// FxHash (the rustc hasher): fast for the short keys used by joins and grouping.
#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.hash = (self.hash.rotate_left(5) ^ w).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap()));
        }
        let rem = chunks.remainder();
        if !rem.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rem.len()].copy_from_slice(rem);
            self.add(u64::from_le_bytes(buf));
        }
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;
pub type FxMap<K, V> = HashMap<K, V, FxBuild>;

// ---------------------------------------------------------------------- context & stats

#[derive(Debug, Clone, Default)]
pub struct OpStats {
    pub rows: usize,
    pub batches: usize,
    pub incl_ms: f64,
}

#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub t_ms: f64,
    pub op: usize,
    pub rows: usize,
}

#[derive(Debug, Clone, Default)]
pub struct PhysNode {
    pub id: usize,
    pub name: String,
    pub detail: String,
    pub children: Vec<PhysNode>,
}

pub struct ExecCtx<'a> {
    pub catalog: &'a Catalog,
    pub int_div: bool,
    pub stats: RefCell<Vec<OpStats>>,
    pub trace: RefCell<Vec<TraceEvent>>,
    pub start: f64,
    pub record: bool,
    /// Row pairs nested-loop joins may still compare before the query is
    /// stopped. Shared with subquery contexts. u64::MAX means no limit.
    pub pair_budget: Rc<Cell<u64>>,
}

impl<'a> ExecCtx<'a> {
    pub fn new(catalog: &'a Catalog, int_div: bool, record: bool) -> Self {
        ExecCtx {
            catalog,
            int_div,
            stats: RefCell::new(Vec::new()),
            trace: RefCell::new(Vec::new()),
            start: now_ms(),
            record,
            pair_budget: Rc::new(Cell::new(u64::MAX)),
        }
    }
    /// A context for subqueries: same catalog and dialect, no instrumentation.
    pub fn child(&self) -> ExecCtx<'a> {
        let mut c = ExecCtx::new(self.catalog, self.int_div, false);
        c.pair_budget = self.pair_budget.clone();
        c
    }
    /// Charge `pairs` row comparisons to the nested-loop budget.
    pub fn charge_pairs(&self, pairs: u64) -> Result<(), String> {
        let left = self.pair_budget.get();
        if left == u64::MAX {
            return Ok(());
        }
        if pairs > left {
            return Err("stopped: this plan compared more row pairs in nested-loop joins than the browser allows. \
                        Without the optimizer, every join is a nested loop over all pairs of rows, because the optimizer \
                        is what turns equality conditions into hash join keys. Turn the optimizer on to run it."
                .to_string());
        }
        self.pair_budget.set(left - pairs);
        Ok(())
    }
}

pub trait Operator {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String>;
}

/// Wraps every physical operator: counts rows/batches, times calls, records the trace.
struct Instrumented {
    id: usize,
    op: Box<dyn Operator>,
}

impl Operator for Instrumented {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if !ctx.record {
            return self.op.next(ctx);
        }
        let t0 = now_ms();
        let r = self.op.next(ctx)?;
        let t1 = now_ms();
        let mut st = ctx.stats.borrow_mut();
        let s = &mut st[self.id];
        s.incl_ms += t1 - t0;
        if let Some(b) = &r {
            s.rows += b.rows;
            s.batches += 1;
            let mut tr = ctx.trace.borrow_mut();
            if tr.len() < 50_000 {
                tr.push(TraceEvent { t_ms: t1 - ctx.start, op: self.id, rows: s.rows });
            }
        }
        Ok(r)
    }
}

fn drain(op: &mut dyn Operator, ctx: &ExecCtx) -> Result<Vec<Batch>, String> {
    let mut out = Vec::new();
    while let Some(b) = op.next(ctx)? {
        if b.rows > 0 {
            out.push(b);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------- operators

struct ScanOp {
    cols: Vec<Rc<Column>>,
    pos: usize,
    total: usize,
}

impl Operator for ScanOp {
    fn next(&mut self, _ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if self.pos >= self.total {
            return Ok(None);
        }
        let len = BATCH_SIZE.min(self.total - self.pos);
        let cols = self.cols.iter().map(|c| slice_column(c, self.pos, len)).collect();
        self.pos += len;
        Ok(Some(Batch::new(cols, len)))
    }
}

pub fn slice_column(c: &Column, start: usize, len: usize) -> Column {
    let r = start..start + len;
    let data = match &c.data {
        ColumnData::Bool(v) => ColumnData::Bool(v[r.clone()].to_vec()),
        ColumnData::Int(v) => ColumnData::Int(v[r.clone()].to_vec()),
        ColumnData::Float(v) => ColumnData::Float(v[r.clone()].to_vec()),
        ColumnData::Date(v) => ColumnData::Date(v[r.clone()].to_vec()),
        ColumnData::Str(v) => {
            let a = v.offsets[start] as usize;
            let b = v.offsets[start + len] as usize;
            let base = v.offsets[start];
            ColumnData::Str(StrVec {
                offsets: v.offsets[start..=start + len].iter().map(|o| o - base).collect(),
                bytes: v.bytes[a..b].to_vec(),
            })
        }
    };
    Column::with_validity(data, c.validity.as_ref().map(|v| v[r].to_vec()))
}

struct ValuesOp {
    batch: Option<Batch>,
}

impl Operator for ValuesOp {
    fn next(&mut self, _ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        Ok(self.batch.take())
    }
}

struct FilterOp {
    input: Box<dyn Operator>,
    pred: PExpr,
}

pub fn selection(mask: &Column) -> Vec<u32> {
    let ColumnData::Bool(v) = &mask.data else { return vec![] };
    let mut idx = Vec::with_capacity(v.len());
    match &mask.validity {
        None => {
            for (i, b) in v.iter().enumerate() {
                if *b {
                    idx.push(i as u32);
                }
            }
        }
        Some(val) => {
            for i in 0..v.len() {
                if v[i] && val[i] {
                    idx.push(i as u32);
                }
            }
        }
    }
    idx
}

impl Operator for FilterOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        loop {
            let Some(b) = self.input.next(ctx)? else { return Ok(None) };
            let mask = eval(&self.pred, &b, ctx)?;
            let mask = if mask.data_type() == DataType::Bool { mask } else { mask.cast(DataType::Bool)? };
            let idx = selection(&mask);
            if idx.is_empty() {
                continue;
            }
            if idx.len() == b.rows {
                return Ok(Some(b));
            }
            return Ok(Some(b.take(&idx)));
        }
    }
}

struct ProjectOp {
    input: Box<dyn Operator>,
    exprs: Vec<PExpr>,
}

impl Operator for ProjectOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        let Some(b) = self.input.next(ctx)? else { return Ok(None) };
        let cols = self.exprs.iter().map(|e| eval(e, &b, ctx)).collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Batch::new(cols, b.rows)))
    }
}

struct LimitOp {
    input: Box<dyn Operator>,
    skip: usize,
    remaining: Option<usize>,
}

impl Operator for LimitOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        loop {
            if self.remaining == Some(0) {
                return Ok(None);
            }
            let Some(mut b) = self.input.next(ctx)? else { return Ok(None) };
            if self.skip > 0 {
                if self.skip >= b.rows {
                    self.skip -= b.rows;
                    continue;
                }
                let idx: Vec<u32> = (self.skip as u32..b.rows as u32).collect();
                b = b.take(&idx);
                self.skip = 0;
            }
            if let Some(r) = self.remaining {
                if b.rows > r {
                    let idx: Vec<u32> = (0..r as u32).collect();
                    b = b.take(&idx);
                }
                self.remaining = Some(r - b.rows);
            }
            return Ok(Some(b));
        }
    }
}

/// Emits a materialized batch in BATCH_SIZE slices.
struct Emitter {
    batch: Batch,
    pos: usize,
}

impl Emitter {
    fn next(&mut self) -> Option<Batch> {
        if self.pos >= self.batch.rows {
            return None;
        }
        let len = BATCH_SIZE.min(self.batch.rows - self.pos);
        let cols = self.batch.columns.iter().map(|c| slice_column(c, self.pos, len)).collect();
        self.pos += len;
        Some(Batch::new(cols, len))
    }
}

// --- sort

/// Float order used by ORDER BY, MIN and MAX: NaN sorts above every number
/// (as in DuckDB) and -0.0 equals 0.0.
#[inline]
pub fn cmp_f64(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => a.partial_cmp(&b).unwrap(),
    }
}

pub fn cmp_cells(c: &Column, i: usize, j: usize) -> Ordering {
    match (c.is_valid(i), c.is_valid(j)) {
        (false, false) => return Ordering::Equal,
        (false, true) => return Ordering::Greater,
        (true, false) => return Ordering::Less,
        _ => {}
    }
    match &c.data {
        ColumnData::Int(v) => v[i].cmp(&v[j]),
        ColumnData::Float(v) => cmp_f64(v[i], v[j]),
        ColumnData::Str(v) => v.get(i).cmp(v.get(j)),
        ColumnData::Date(v) => v[i].cmp(&v[j]),
        ColumnData::Bool(v) => v[i].cmp(&v[j]),
    }
}

struct SortOp {
    input: Option<Box<dyn Operator>>,
    keys: Vec<(PExpr, bool, bool)>,
    types: Vec<DataType>,
    out: Option<Emitter>,
}

impl Operator for SortOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if let Some(mut input) = self.input.take() {
            let batches = drain(input.as_mut(), ctx)?;
            let all = Batch::concat(&batches, &self.types);
            let keys: Vec<Column> = self.keys.iter().map(|(e, _, _)| eval(e, &all, ctx)).collect::<Result<_, _>>()?;
            let mut idx: Vec<u32> = (0..all.rows as u32).collect();
            let dirs: Vec<(bool, bool)> = self.keys.iter().map(|(_, a, nf)| (*a, *nf)).collect();
            idx.sort_by(|&a, &b| {
                for (k, (asc, nulls_first)) in keys.iter().zip(&dirs) {
                    let (a, b) = (a as usize, b as usize);
                    let (va, vb) = (k.is_valid(a), k.is_valid(b));
                    let o = if !va || !vb {
                        match (va, vb) {
                            (false, false) => Ordering::Equal,
                            (false, true) => if *nulls_first { Ordering::Less } else { Ordering::Greater },
                            _ => if *nulls_first { Ordering::Greater } else { Ordering::Less },
                        }
                    } else {
                        let o = cmp_cells(k, a, b);
                        if *asc { o } else { o.reverse() }
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                Ordering::Equal
            });
            self.out = Some(Emitter { batch: all.take(&idx), pos: 0 });
        }
        Ok(self.out.as_mut().and_then(|e| e.next()))
    }
}

// --- hash aggregate

enum Acc {
    CountStar(Vec<i64>),
    Count(Vec<i64>),
    SumInt(Vec<i64>, Vec<bool>),
    SumFloat(Vec<f64>, Vec<bool>),
    Avg(Vec<f64>, Vec<i64>),
    MinMax(Vec<Value>, bool),
    Distinct(Vec<FxMap<Vec<u8>, Value>>, AggFunc, DataType),
}

impl Acc {
    fn new(func: AggFunc, distinct: bool, in_type: DataType) -> Acc {
        if distinct && func != AggFunc::Min && func != AggFunc::Max {
            return Acc::Distinct(Vec::new(), func, in_type);
        }
        match func {
            AggFunc::CountStar => Acc::CountStar(Vec::new()),
            AggFunc::Count => Acc::Count(Vec::new()),
            AggFunc::Sum if in_type == DataType::Int || in_type == DataType::Null => Acc::SumInt(Vec::new(), Vec::new()),
            AggFunc::Sum => Acc::SumFloat(Vec::new(), Vec::new()),
            AggFunc::Avg => Acc::Avg(Vec::new(), Vec::new()),
            AggFunc::Min => Acc::MinMax(Vec::new(), true),
            AggFunc::Max => Acc::MinMax(Vec::new(), false),
        }
    }
    fn grow(&mut self, n: usize) {
        match self {
            Acc::CountStar(v) | Acc::Count(v) => v.resize(n, 0),
            Acc::SumInt(v, s) => {
                v.resize(n, 0);
                s.resize(n, false)
            }
            Acc::SumFloat(v, s) => {
                v.resize(n, 0.0);
                s.resize(n, false)
            }
            Acc::Avg(v, c) => {
                v.resize(n, 0.0);
                c.resize(n, 0)
            }
            Acc::MinMax(v, _) => v.resize(n, Value::Null),
            Acc::Distinct(v, _, _) => v.resize_with(n, FxMap::default),
        }
    }
    fn update(&mut self, gids: &[u32], col: Option<&Column>) {
        match self {
            Acc::CountStar(v) => {
                for &g in gids {
                    v[g as usize] += 1;
                }
            }
            Acc::Count(v) => {
                let c = col.unwrap();
                for (i, &g) in gids.iter().enumerate() {
                    if c.is_valid(i) {
                        v[g as usize] += 1;
                    }
                }
            }
            Acc::SumInt(v, seen) => {
                let c = col.unwrap();
                if let ColumnData::Int(x) = &c.data {
                    for (i, &g) in gids.iter().enumerate() {
                        if c.is_valid(i) {
                            v[g as usize] = v[g as usize].wrapping_add(x[i]);
                            seen[g as usize] = true;
                        }
                    }
                }
            }
            Acc::SumFloat(v, seen) => {
                let c = col.unwrap();
                let x = as_f64_vec(c);
                for (i, &g) in gids.iter().enumerate() {
                    if c.is_valid(i) {
                        v[g as usize] += x[i];
                        seen[g as usize] = true;
                    }
                }
            }
            Acc::Avg(v, cnt) => {
                let c = col.unwrap();
                let x = as_f64_vec(c);
                for (i, &g) in gids.iter().enumerate() {
                    if c.is_valid(i) {
                        v[g as usize] += x[i];
                        cnt[g as usize] += 1;
                    }
                }
            }
            Acc::MinMax(v, is_min) => {
                let c = col.unwrap();
                for (i, &g) in gids.iter().enumerate() {
                    if !c.is_valid(i) {
                        continue;
                    }
                    let cur = &mut v[g as usize];
                    let better = match (&*cur, &c.data) {
                        (Value::Null, _) => true,
                        (Value::Int(a), ColumnData::Int(x)) => if *is_min { x[i] < *a } else { x[i] > *a },
                        (Value::Float(a), ColumnData::Float(x)) => cmp_f64(x[i], *a) == if *is_min { Ordering::Less } else { Ordering::Greater },
                        (Value::Date(a), ColumnData::Date(x)) => if *is_min { x[i] < *a } else { x[i] > *a },
                        (Value::Str(a), ColumnData::Str(x)) => if *is_min { x.get(i) < a.as_str() } else { x.get(i) > a.as_str() },
                        (cv, _) => {
                            let o = c.get(i).total_cmp(cv);
                            if *is_min { o == Ordering::Less } else { o == Ordering::Greater }
                        }
                    };
                    if better {
                        *cur = c.get(i);
                    }
                }
            }
            Acc::Distinct(sets, _, _) => {
                let c = col.unwrap();
                let mut buf = Vec::new();
                for (i, &g) in gids.iter().enumerate() {
                    if !c.is_valid(i) {
                        continue;
                    }
                    buf.clear();
                    encode_cell(c, i, &mut buf);
                    if !sets[g as usize].contains_key(&buf) {
                        sets[g as usize].insert(buf.clone(), c.get(i));
                    }
                }
            }
        }
    }
    fn finish(self, out_type: DataType) -> Column {
        match self {
            Acc::CountStar(v) | Acc::Count(v) => Column::new(ColumnData::Int(v)),
            Acc::SumInt(v, s) => Column::with_validity(ColumnData::Int(v), Some(s)),
            Acc::SumFloat(v, s) => Column::with_validity(ColumnData::Float(v), Some(s)),
            Acc::Avg(v, c) => {
                let valid: Vec<bool> = c.iter().map(|x| *x > 0).collect();
                let out = v.iter().zip(&c).map(|(s, n)| if *n > 0 { s / *n as f64 } else { 0.0 }).collect();
                Column::with_validity(ColumnData::Float(out), Some(valid))
            }
            Acc::MinMax(v, _) => {
                let mut b = ColumnBuilder::new(out_type, v.len());
                for x in &v {
                    b.push(x);
                }
                b.finish()
            }
            Acc::Distinct(sets, func, in_type) => {
                let mut b = ColumnBuilder::new(out_type, sets.len());
                for s in sets {
                    let vals: Vec<Value> = s.into_values().collect();
                    let v = match func {
                        AggFunc::Count => Value::Int(vals.len() as i64),
                        _ if vals.is_empty() => Value::Null,
                        AggFunc::Sum if in_type == DataType::Int => {
                            Value::Int(vals.iter().map(|x| if let Value::Int(i) = x { *i } else { 0 }).sum())
                        }
                        AggFunc::Sum => Value::Float(vals.iter().filter_map(|x| x.as_f64()).sum()),
                        AggFunc::Avg => Value::Float(vals.iter().filter_map(|x| x.as_f64()).sum::<f64>() / vals.len() as f64),
                        _ => Value::Null,
                    };
                    b.push(&v);
                }
                b.finish()
            }
        }
    }
}

fn as_f64_vec(c: &Column) -> std::borrow::Cow<'_, [f64]> {
    match &c.data {
        ColumnData::Float(v) => std::borrow::Cow::Borrowed(v),
        ColumnData::Int(v) => std::borrow::Cow::Owned(v.iter().map(|x| *x as f64).collect()),
        _ => std::borrow::Cow::Owned(vec![0.0; c.len()]),
    }
}

struct AggSpec {
    func: AggFunc,
    distinct: bool,
    arg: Option<PExpr>,
    in_type: DataType,
    out_type: DataType,
}

struct HashAggOp {
    input: Option<Box<dyn Operator>>,
    group: Vec<PExpr>,
    group_types: Vec<DataType>,
    aggs: Vec<AggSpec>,
    out: Option<Emitter>,
}

impl Operator for HashAggOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if let Some(mut input) = self.input.take() {
            let mut accs: Vec<Acc> = self.aggs.iter().map(|a| Acc::new(a.func, a.distinct, a.in_type)).collect();
            let mut keys: Vec<ColumnBuilder> = self.group_types.iter().map(|t| ColumnBuilder::new(*t, 1024)).collect();
            let mut ngroups = 0usize;
            let mut map: FxMap<Vec<u8>, u32> = FxMap::default();
            let mut int_map: FxMap<i64, u32> = FxMap::default();
            let single_int = self.group_types.len() == 1 && self.group_types[0] == DataType::Int;
            if self.group.is_empty() {
                ngroups = 1;
                for a in accs.iter_mut() {
                    a.grow(1);
                }
            }
            let mut buf = Vec::with_capacity(64);
            while let Some(b) = input.next(ctx)? {
                if b.rows == 0 {
                    continue;
                }
                let gcols: Vec<Column> = self.group.iter().map(|e| eval(e, &b, ctx)).collect::<Result<_, _>>()?;
                let mut gids = Vec::with_capacity(b.rows);
                if self.group.is_empty() {
                    gids.resize(b.rows, 0u32);
                } else if single_int && gcols[0].validity.is_none() {
                    let ColumnData::Int(v) = &gcols[0].data else { unreachable!() };
                    for i in 0..b.rows {
                        let id = *int_map.entry(v[i]).or_insert_with(|| {
                            keys[0].push_from(&gcols[0], i);
                            ngroups += 1;
                            (ngroups - 1) as u32
                        });
                        gids.push(id);
                    }
                } else {
                    for i in 0..b.rows {
                        buf.clear();
                        for c in &gcols {
                            encode_cell(c, i, &mut buf);
                        }
                        let id = match map.get(buf.as_slice()) {
                            Some(id) => *id,
                            None => {
                                for (k, c) in gcols.iter().enumerate() {
                                    keys[k].push_from(c, i);
                                }
                                ngroups += 1;
                                map.insert(buf.clone(), (ngroups - 1) as u32);
                                (ngroups - 1) as u32
                            }
                        };
                        gids.push(id);
                    }
                }
                for (a, spec) in accs.iter_mut().zip(&self.aggs) {
                    a.grow(ngroups);
                    let col = match &spec.arg {
                        Some(e) => Some(eval(e, &b, ctx)?),
                        None => None,
                    };
                    a.update(&gids, col.as_ref());
                }
            }
            if single_int && !int_map.is_empty() && !map.is_empty() {
                return Err("internal: mixed group maps".into());
            }
            for a in accs.iter_mut() {
                a.grow(ngroups);
            }
            let mut cols: Vec<Column> = keys.into_iter().map(|k| k.finish()).collect();
            for (a, spec) in accs.into_iter().zip(&self.aggs) {
                cols.push(a.finish(spec.out_type));
            }
            self.out = Some(Emitter { batch: Batch::new(cols, ngroups), pos: 0 });
        }
        Ok(self.out.as_mut().and_then(|e| e.next()))
    }
}

// --- hash join

/// Build side of a hash join: rows chained by key.
struct HashTable {
    batch: Batch,
    int_heads: FxMap<i64, u32>,
    heads: FxMap<Vec<u8>, u32>,
    next: Vec<u32>,
    single_int: bool,
}

const NIL: u32 = u32::MAX;

struct HashJoinOp {
    left: Box<dyn Operator>,
    right: Option<Box<dyn Operator>>,
    kind: JoinType,
    left_keys: Vec<PExpr>,
    right_keys: Vec<PExpr>,
    key_types: Vec<DataType>,
    residual: Option<PExpr>,
    right_types: Vec<DataType>,
    table: Option<HashTable>,
}

fn key_cols(exprs: &[PExpr], types: &[DataType], b: &Batch, ctx: &ExecCtx) -> Result<Vec<Column>, String> {
    exprs
        .iter()
        .zip(types)
        .map(|(e, t)| {
            let c = eval(e, b, ctx)?;
            if c.data_type() == *t { Ok(c) } else { c.cast(*t) }
        })
        .collect()
}

impl HashJoinOp {
    fn build(&mut self, ctx: &ExecCtx) -> Result<(), String> {
        let mut right = self.right.take().unwrap();
        let batches = drain(right.as_mut(), ctx)?;
        let batch = Batch::concat(&batches, &self.right_types);
        let keys = key_cols(&self.right_keys, &self.key_types, &batch, ctx)?;
        let single_int = keys.len() == 1 && keys[0].data_type() == DataType::Int;
        let mut t = HashTable { batch, int_heads: FxMap::default(), heads: FxMap::default(), next: vec![NIL; 0], single_int };
        let n = t.batch.rows;
        t.next = vec![NIL; n];
        let mut buf = Vec::with_capacity(64);
        // insert in reverse so chains come out in input order
        for i in (0..n).rev() {
            if keys.iter().any(|k| !k.is_valid(i)) {
                continue;
            }
            if single_int {
                let ColumnData::Int(v) = &keys[0].data else { unreachable!() };
                let h = t.int_heads.entry(v[i]).or_insert(NIL);
                t.next[i] = *h;
                *h = i as u32;
            } else {
                buf.clear();
                for k in &keys {
                    encode_cell(k, i, &mut buf);
                }
                let h = t.heads.entry(buf.clone()).or_insert(NIL);
                t.next[i] = *h;
                *h = i as u32;
            }
        }
        self.table = Some(t);
        Ok(())
    }
}

impl Operator for HashJoinOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if self.table.is_none() {
            self.build(ctx)?;
        }
        loop {
            let Some(b) = self.left.next(ctx)? else { return Ok(None) };
            let t = self.table.as_ref().unwrap();
            if t.batch.rows == 0 {
                match self.kind {
                    JoinType::Inner | JoinType::Semi => continue,
                    JoinType::Anti => return Ok(Some(b)),
                    JoinType::Left => {
                        let mut cols = b.columns.clone();
                        for ty in &self.right_types {
                            cols.push(Column::nulls(*ty, b.rows));
                        }
                        return Ok(Some(Batch::new(cols, b.rows)));
                    }
                    JoinType::Cross => unreachable!(),
                }
            }
            let keys = key_cols(&self.left_keys, &self.key_types, &b, ctx)?;
            let mut li: Vec<u32> = Vec::with_capacity(b.rows);
            let mut ri: Vec<u32> = Vec::with_capacity(b.rows);
            let mut buf = Vec::with_capacity(64);
            for i in 0..b.rows {
                if keys.iter().any(|k| !k.is_valid(i)) {
                    continue;
                }
                let mut h = if t.single_int {
                    let ColumnData::Int(v) = &keys[0].data else { unreachable!() };
                    *t.int_heads.get(&v[i]).unwrap_or(&NIL)
                } else {
                    buf.clear();
                    for k in &keys {
                        encode_cell(k, i, &mut buf);
                    }
                    *t.heads.get(buf.as_slice()).unwrap_or(&NIL)
                };
                while h != NIL {
                    li.push(i as u32);
                    ri.push(h);
                    h = t.next[h as usize];
                }
            }
            // residual predicate on candidate pairs
            let mut pair_batch: Option<Batch> = None;
            if let Some(res) = &self.residual {
                if !li.is_empty() {
                    let mut cols: Vec<Column> = b.columns.iter().map(|c| c.take(&li)).collect();
                    cols.extend(t.batch.columns.iter().map(|c| c.take(&ri)));
                    let pb = Batch::new(cols, li.len());
                    let mask = eval(res, &pb, ctx)?;
                    let keep = selection(&mask);
                    if keep.len() != li.len() {
                        li = keep.iter().map(|&k| li[k as usize]).collect();
                        ri = keep.iter().map(|&k| ri[k as usize]).collect();
                        if self.kind == JoinType::Inner {
                            pair_batch = Some(pb.take(&keep));
                        }
                    } else if self.kind == JoinType::Inner {
                        pair_batch = Some(pb);
                    }
                }
            }
            let out = match self.kind {
                JoinType::Inner => {
                    if li.is_empty() {
                        continue;
                    }
                    match pair_batch {
                        Some(pb) => pb,
                        None => {
                            let mut cols: Vec<Column> = b.columns.iter().map(|c| c.take(&li)).collect();
                            cols.extend(t.batch.columns.iter().map(|c| c.take(&ri)));
                            Batch::new(cols, li.len())
                        }
                    }
                }
                JoinType::Semi | JoinType::Anti => {
                    let mut matched = vec![false; b.rows];
                    for &l in &li {
                        matched[l as usize] = true;
                    }
                    let want = self.kind == JoinType::Semi;
                    let idx: Vec<u32> = (0..b.rows as u32).filter(|&i| matched[i as usize] == want).collect();
                    if idx.is_empty() {
                        continue;
                    }
                    b.take(&idx)
                }
                JoinType::Left => {
                    let mut matched = vec![false; b.rows];
                    for &l in &li {
                        matched[l as usize] = true;
                    }
                    let mut lidx = li.clone();
                    let mut ridx: Vec<Option<u32>> = ri.iter().map(|r| Some(*r)).collect();
                    for i in 0..b.rows {
                        if !matched[i] {
                            lidx.push(i as u32);
                            ridx.push(None);
                        }
                    }
                    let mut cols: Vec<Column> = b.columns.iter().map(|c| c.take(&lidx)).collect();
                    cols.extend(t.batch.columns.iter().map(|c| c.take_opt(&ridx)));
                    Batch::new(cols, lidx.len())
                }
                JoinType::Cross => unreachable!(),
            };
            return Ok(Some(out));
        }
    }
}

// --- nested loop join (cross products and joins without equality keys)

struct NestedLoopOp {
    left: Box<dyn Operator>,
    right: Option<Box<dyn Operator>>,
    kind: JoinType,
    filter: Option<PExpr>,
    right_types: Vec<DataType>,
    rbatch: Option<Batch>,
    pending: Option<(Batch, usize)>,
    // for LEFT / SEMI / ANTI we process a whole left batch at once
}

impl Operator for NestedLoopOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if self.rbatch.is_none() {
            let mut r = self.right.take().unwrap();
            let batches = drain(r.as_mut(), ctx)?;
            self.rbatch = Some(Batch::concat(&batches, &self.right_types));
        }
        let rn = self.rbatch.as_ref().unwrap().rows;
        loop {
            if self.pending.is_none() {
                let Some(b) = self.left.next(ctx)? else { return Ok(None) };
                if b.rows == 0 {
                    continue;
                }
                self.pending = Some((b, 0));
            }
            let (lb, start) = self.pending.take().unwrap();
            let rb = self.rbatch.as_ref().unwrap();
            let inner_like = matches!(self.kind, JoinType::Inner | JoinType::Cross);
            // chunk of left rows so the pair count stays bounded
            let chunk = if inner_like { (BATCH_SIZE * 4 / rn.max(1)).max(1) } else { lb.rows };
            let end = (start + chunk).min(lb.rows);
            if end < lb.rows {
                self.pending = Some((lb.clone(), end));
            }
            ctx.charge_pairs(((end - start) * rn) as u64)?;
            let mut li = Vec::new();
            let mut ri = Vec::new();
            for l in start..end {
                for r in 0..rn {
                    li.push(l as u32);
                    ri.push(r as u32);
                }
            }
            let mut keep_l = li.clone();
            let mut keep_r = ri.clone();
            if let Some(f) = &self.filter {
                if !li.is_empty() {
                    let mut cols: Vec<Column> = lb.columns.iter().map(|c| c.take(&li)).collect();
                    cols.extend(rb.columns.iter().map(|c| c.take(&ri)));
                    let pb = Batch::new(cols, li.len());
                    let keep = selection(&eval(f, &pb, ctx)?);
                    keep_l = keep.iter().map(|&k| li[k as usize]).collect();
                    keep_r = keep.iter().map(|&k| ri[k as usize]).collect();
                }
            }
            let out = match self.kind {
                JoinType::Inner | JoinType::Cross => {
                    if keep_l.is_empty() {
                        continue;
                    }
                    let mut cols: Vec<Column> = lb.columns.iter().map(|c| c.take(&keep_l)).collect();
                    cols.extend(rb.columns.iter().map(|c| c.take(&keep_r)));
                    Batch::new(cols, keep_l.len())
                }
                JoinType::Semi | JoinType::Anti => {
                    let mut matched = vec![false; lb.rows];
                    for &l in &keep_l {
                        matched[l as usize] = true;
                    }
                    let want = self.kind == JoinType::Semi;
                    let idx: Vec<u32> = (0..lb.rows as u32).filter(|&i| matched[i as usize] == want).collect();
                    if idx.is_empty() {
                        continue;
                    }
                    lb.take(&idx)
                }
                JoinType::Left => {
                    let mut matched = vec![false; lb.rows];
                    for &l in &keep_l {
                        matched[l as usize] = true;
                    }
                    let mut lidx = keep_l.clone();
                    let mut ridx: Vec<Option<u32>> = keep_r.iter().map(|r| Some(*r)).collect();
                    for i in 0..lb.rows {
                        if !matched[i] {
                            lidx.push(i as u32);
                            ridx.push(None);
                        }
                    }
                    let mut cols: Vec<Column> = lb.columns.iter().map(|c| c.take(&lidx)).collect();
                    cols.extend(rb.columns.iter().map(|c| c.take_opt(&ridx)));
                    Batch::new(cols, lidx.len())
                }
            };
            return Ok(Some(out));
        }
    }
}

// --- set operations

struct UnionOp {
    inputs: Vec<Box<dyn Operator>>,
    types: Vec<DataType>,
    distinct: bool,
    seen: HashSet<Vec<u8>, FxBuild>,
}

fn cast_batch(b: Batch, types: &[DataType]) -> Result<Batch, String> {
    let rows = b.rows;
    let cols = b
        .columns
        .into_iter()
        .zip(types)
        .map(|(c, t)| if c.data_type() == *t || *t == DataType::Null { Ok(c) } else { c.cast(*t) })
        .collect::<Result<_, _>>()?;
    Ok(Batch::new(cols, rows))
}

fn dedup(b: Batch, seen: &mut HashSet<Vec<u8>, FxBuild>) -> Batch {
    let mut keep = Vec::new();
    let mut buf = Vec::new();
    for i in 0..b.rows {
        buf.clear();
        for c in &b.columns {
            encode_cell(c, i, &mut buf);
        }
        if seen.insert(buf.clone()) {
            keep.push(i as u32);
        }
    }
    b.take(&keep)
}

impl Operator for UnionOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        while !self.inputs.is_empty() {
            match self.inputs[0].next(ctx)? {
                None => {
                    self.inputs.remove(0);
                }
                Some(b) => {
                    let b = cast_batch(b, &self.types)?;
                    let b = if self.distinct { dedup(b, &mut self.seen) } else { b };
                    if b.rows > 0 {
                        return Ok(Some(b));
                    }
                }
            }
        }
        Ok(None)
    }
}

struct SetOpOp {
    left: Box<dyn Operator>,
    right: Option<Box<dyn Operator>>,
    intersect: bool,
    types: Vec<DataType>,
    right_set: HashSet<Vec<u8>, FxBuild>,
    seen: HashSet<Vec<u8>, FxBuild>,
}

impl Operator for SetOpOp {
    fn next(&mut self, ctx: &ExecCtx) -> Result<Option<Batch>, String> {
        if let Some(mut r) = self.right.take() {
            while let Some(b) = r.next(ctx)? {
                let b = cast_batch(b, &self.types)?;
                let mut buf = Vec::new();
                for i in 0..b.rows {
                    buf.clear();
                    for c in &b.columns {
                        encode_cell(c, i, &mut buf);
                    }
                    self.right_set.insert(buf.clone());
                }
            }
        }
        loop {
            let Some(b) = self.left.next(ctx)? else { return Ok(None) };
            let b = cast_batch(b, &self.types)?;
            let mut keep = Vec::new();
            let mut buf = Vec::new();
            for i in 0..b.rows {
                buf.clear();
                for c in &b.columns {
                    encode_cell(c, i, &mut buf);
                }
                if self.right_set.contains(&buf) == self.intersect && self.seen.insert(buf.clone()) {
                    keep.push(i as u32);
                }
            }
            if !keep.is_empty() {
                return Ok(Some(b.take(&keep)));
            }
        }
    }
}

// ---------------------------------------------------------------------- physical planning

pub struct PhysicalPlanner<'c, 'a> {
    pub ctx: &'c ExecCtx<'a>,
    next_id: usize,
}

impl<'c, 'a> PhysicalPlanner<'c, 'a> {
    pub fn new(ctx: &'c ExecCtx<'a>) -> Self {
        PhysicalPlanner { ctx, next_id: 0 }
    }

    fn wrap(&mut self, op: Box<dyn Operator>, name: &str, detail: String, children: Vec<PhysNode>) -> (Box<dyn Operator>, PhysNode) {
        let id = self.next_id;
        self.next_id += 1;
        self.ctx.stats.borrow_mut().push(OpStats::default());
        (Box::new(Instrumented { id, op }), PhysNode { id, name: name.into(), detail, children })
    }

    pub fn build(&mut self, plan: &LogicalPlan) -> Result<(Box<dyn Operator>, PhysNode), String> {
        let (label, detail) = plan.node_label();
        match plan {
            LogicalPlan::Scan { table, projection, .. } => {
                let t = self.ctx.catalog.get(table).ok_or_else(|| format!("Catalog error: table {} not found", table))?;
                let idx: Vec<usize> = match projection {
                    Some(p) => p.clone(),
                    None => (0..t.columns.len()).collect(),
                };
                let cols = idx.iter().map(|&i| t.data[i].clone()).collect();
                let op = ScanOp { cols, pos: 0, total: t.rows };
                Ok(self.wrap(Box::new(op), "SEQ_SCAN", detail, vec![]))
            }
            LogicalPlan::Values { schema, rows } => {
                let empty = Batch::new(vec![], 1);
                let mut builders: Vec<ColumnBuilder> = schema.fields.iter().map(|f| ColumnBuilder::new(f.dtype, rows.len())).collect();
                for r in rows {
                    for (k, e) in r.iter().enumerate() {
                        let pe = compile(e, &Schema::default())?;
                        let c = eval(&pe, &empty, self.ctx)?;
                        builders[k].push(&c.get(0));
                    }
                }
                let cols: Vec<Column> = builders.into_iter().map(|b| b.finish()).collect();
                let op = ValuesOp { batch: Some(Batch::new(cols, rows.len())) };
                Ok(self.wrap(Box::new(op), "VALUES", detail, vec![]))
            }
            LogicalPlan::Filter { input, predicate } => {
                let (child, node) = self.build(input)?;
                let pred = compile(predicate, &input.schema())?;
                Ok(self.wrap(Box::new(FilterOp { input: child, pred }), "FILTER", detail, vec![node]))
            }
            LogicalPlan::Project { input, exprs, .. } => {
                let (child, node) = self.build(input)?;
                let s = input.schema();
                let exprs = exprs.iter().map(|e| compile(e, &s)).collect::<Result<_, _>>()?;
                Ok(self.wrap(Box::new(ProjectOp { input: child, exprs }), "PROJECTION", detail, vec![node]))
            }
            LogicalPlan::Alias { input, .. } => self.build(input),
            LogicalPlan::Limit { input, limit, offset } => {
                let (child, node) = self.build(input)?;
                Ok(self.wrap(Box::new(LimitOp { input: child, skip: *offset, remaining: *limit }), "LIMIT", detail, vec![node]))
            }
            LogicalPlan::Sort { input, keys } => {
                let (child, node) = self.build(input)?;
                let s = input.schema();
                let keys = keys.iter().map(|k| Ok((compile(&k.expr, &s)?, k.asc, k.nulls_first))).collect::<Result<_, String>>()?;
                let types = s.fields.iter().map(|f| f.dtype).collect();
                Ok(self.wrap(Box::new(SortOp { input: Some(child), keys, types, out: None }), "ORDER_BY", detail, vec![node]))
            }
            LogicalPlan::Aggregate { input, group, aggs, schema } => {
                let (child, node) = self.build(input)?;
                let s = input.schema();
                let group_c = group.iter().map(|e| compile(e, &s)).collect::<Result<_, _>>()?;
                let group_types = schema.fields[..group.len()].iter().map(|f| if f.dtype == DataType::Null { DataType::Int } else { f.dtype }).collect();
                let mut specs = Vec::new();
                for (k, a) in aggs.iter().enumerate() {
                    let Expr::Agg { func, arg, distinct } = a else { return Err("internal: expected aggregate".into()) };
                    let (argc, in_type) = match arg {
                        Some(x) => (Some(compile(x, &s)?), expr_type(x, &s, None)?),
                        None => (None, DataType::Int),
                    };
                    specs.push(AggSpec { func: *func, distinct: *distinct, arg: argc, in_type, out_type: schema.fields[group.len() + k].dtype });
                }
                let op = HashAggOp { input: Some(child), group: group_c, group_types, aggs: specs, out: None };
                let name = if group.is_empty() { "UNGROUPED_AGGREGATE" } else { "HASH_GROUP_BY" };
                Ok(self.wrap(Box::new(op), name, detail, vec![node]))
            }
            LogicalPlan::Join { left, right, kind, on, filter } => {
                let (l, ln) = self.build(left)?;
                let (r, rn) = self.build(right)?;
                let ls = left.schema();
                let rs = right.schema();
                let joined = ls.join(&rs);
                let right_types: Vec<DataType> = rs.fields.iter().map(|f| f.dtype).collect();
                let residual = match filter {
                    Some(f) => Some(compile(f, &joined)?),
                    None => None,
                };
                if on.is_empty() {
                    let op = NestedLoopOp { left: l, right: Some(r), kind: *kind, filter: residual, right_types, rbatch: None, pending: None };
                    let name = if *kind == JoinType::Cross { "CROSS_PRODUCT".to_string() } else { format!("NESTED_LOOP_JOIN") };
                    let _ = label;
                    return Ok(self.wrap(Box::new(op), &name, detail, vec![ln, rn]));
                }
                let mut left_keys = Vec::new();
                let mut right_keys = Vec::new();
                let mut key_types = Vec::new();
                for (a, b) in on {
                    left_keys.push(compile(a, &ls)?);
                    right_keys.push(compile(b, &rs)?);
                    let ta = expr_type(a, &ls, None)?;
                    let tb = expr_type(b, &rs, None)?;
                    let t = unify(ta, tb);
                    key_types.push(if t == DataType::Null { DataType::Int } else { t });
                }
                let op = HashJoinOp { left: l, right: Some(r), kind: *kind, left_keys, right_keys, key_types, residual, right_types, table: None };
                Ok(self.wrap(Box::new(op), "HASH_JOIN", detail, vec![ln, rn]))
            }
            LogicalPlan::Union { left, right, all, schema } => {
                let (l, ln) = self.build(left)?;
                let (r, rn) = self.build(right)?;
                let types = schema.fields.iter().map(|f| f.dtype).collect();
                let op = UnionOp { inputs: vec![l, r], types, distinct: !*all, seen: HashSet::default() };
                Ok(self.wrap(Box::new(op), if *all { "UNION_ALL" } else { "UNION" }, detail, vec![ln, rn]))
            }
            LogicalPlan::SetOp { left, right, intersect, schema } => {
                let (l, ln) = self.build(left)?;
                let (r, rn) = self.build(right)?;
                let types = schema.fields.iter().map(|f| f.dtype).collect();
                let op = SetOpOp { left: l, right: Some(r), intersect: *intersect, types, right_set: HashSet::default(), seen: HashSet::default() };
                Ok(self.wrap(Box::new(op), if *intersect { "INTERSECT" } else { "EXCEPT" }, detail, vec![ln, rn]))
            }
        }
    }
}

/// Optimize, plan and run a (sub)plan to completion. Returns columns and row count.
pub fn run_plan_rows(plan: &LogicalPlan, ctx: &ExecCtx, limit: Option<usize>) -> Result<(Vec<Column>, usize), String> {
    let plan = crate::optimizer::optimize(plan.clone(), ctx.catalog, ctx.int_div)?;
    let plan = match limit {
        Some(l) => LogicalPlan::Limit { input: Box::new(plan), limit: Some(l), offset: 0 },
        None => plan,
    };
    let sub = ctx.child();
    let mut pp = PhysicalPlanner::new(&sub);
    let (mut op, _) = pp.build(&plan)?;
    let types: Vec<DataType> = plan.schema().fields.iter().map(|f| f.dtype).collect();
    let batches = drain(op.as_mut(), &sub)?;
    let all = Batch::concat(&batches, &types);
    Ok((all.columns, all.rows))
}

/// Evaluate a constant expression (no column references).
pub fn eval_const(e: &Expr) -> Result<Value, String> {
    eval_const_mode(e, false)
}

/// Evaluate a constant expression under the given division semantics.
pub fn eval_const_mode(e: &Expr, int_div: bool) -> Result<Value, String> {
    let cat = Catalog::default();
    let ctx = ExecCtx::new(&cat, int_div, false);
    let pe = compile(e, &Schema::default())?;
    let c = eval(&pe, &Batch::new(vec![], 1), &ctx)?;
    Ok(c.get(0))
}

pub fn collect(op: &mut dyn Operator, ctx: &ExecCtx, types: &[DataType]) -> Result<Batch, String> {
    let batches = drain(op, ctx)?;
    Ok(Batch::concat(&batches, types))
}
