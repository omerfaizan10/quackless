//! Quackless: a columnar, vectorized SQL engine.
//!
//! Pipeline: `lexer` -> `parser` (AST) -> `planner` (logical plan) ->
//! `optimizer` (rule-based rewrites) -> `exec` (physical, pull-based
//! operators over column batches).

pub mod ast;
pub mod catalog;
pub mod eval;
pub mod exec;
pub mod expr;
pub mod functions;
pub mod json;
pub mod lexer;
pub mod optimizer;
pub mod parser;
pub mod plan;
pub mod planner;
pub mod types;

use ast::{InsertSource, Statement};
use catalog::{Catalog, Table};
use exec::{collect, now_ms, ExecCtx, OpStats, PhysNode, PhysicalPlanner, TraceEvent};
use plan::{LogicalPlan, Schema};
use types::{cast_value, Batch, Column, ColumnBuilder, DataType, Value};

/// Execution options.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Run the rule-based optimizer (the workbench can switch it off).
    pub optimize: bool,
    /// Integer / integer division truncates (SQLite) instead of producing a
    /// double (DuckDB). sqllogictest files are written for the former.
    pub int_div: bool,
    /// Collect per-operator row counts, timings and the trace.
    pub record: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { optimize: true, int_div: false, record: false }
    }
}

/// The result of one statement.
#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    pub names: Vec<String>,
    pub types: Vec<DataType>,
    pub columns: Vec<Column>,
    pub rows: usize,
    /// Logical plan straight from the planner.
    pub plan_before: Option<LogicalPlan>,
    /// Logical plan after the optimizer (same as before when it is off).
    pub plan_after: Option<LogicalPlan>,
    pub physical: Option<PhysNode>,
    pub stats: Vec<OpStats>,
    pub trace: Vec<TraceEvent>,
    /// Rule name and what it changed.
    pub opt_log: Vec<(String, String)>,
    pub parse_ms: f64,
    pub plan_ms: f64,
    pub optimize_ms: f64,
    pub exec_ms: f64,
    /// Row count for DDL / INSERT statements.
    pub affected: Option<usize>,
}

impl QueryResult {
    pub fn value(&self, row: usize, col: usize) -> Value {
        self.columns[col].get(row)
    }

    /// Rows as display strings (NULL shown as "NULL").
    pub fn string_rows(&self) -> Vec<Vec<String>> {
        (0..self.rows)
            .map(|r| self.columns.iter().map(|c| c.get(r).to_string()).collect())
            .collect()
    }
}

/// An in-memory database.
#[derive(Default)]
pub struct Database {
    pub catalog: Catalog,
    /// Most row pairs nested-loop joins may compare in one query (None: no limit).
    /// The browser sets this so a cross product cannot hang the page.
    pub pair_budget: Option<u64>,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load CSV text (with a header row) into an existing table, replacing its rows.
    pub fn load_csv(&mut self, table: &str, text: &[u8]) -> Result<usize, String> {
        let t = self.catalog.get(table).ok_or_else(|| format!("Catalog error: table {} does not exist", table))?;
        let loaded = catalog::load_csv(table, &t.columns.clone(), text)?;
        let n = loaded.rows;
        self.catalog.tables.insert(table.to_string(), loaded);
        self.catalog.invalidate(table);
        Ok(n)
    }

    /// Run every statement in a script; returns the last result.
    pub fn execute_script(&mut self, sql: &str, opts: Options) -> Result<QueryResult, String> {
        let stmts = parser::parse_statements(sql)?;
        let mut last = QueryResult::default();
        for s in stmts {
            last = self.run_statement(s, opts, 0.0)?;
        }
        Ok(last)
    }

    /// Parse and run exactly one statement.
    pub fn execute(&mut self, sql: &str, opts: Options) -> Result<QueryResult, String> {
        let t0 = now_ms();
        let stmt = parser::parse_statement(sql)?;
        let parse_ms = now_ms() - t0;
        self.run_statement(stmt, opts, parse_ms)
    }

    /// Run a read-only statement (SELECT or EXPLAIN) without needing `&mut self`.
    pub fn query(&self, sql: &str, opts: Options) -> Result<QueryResult, String> {
        let t0 = now_ms();
        let stmt = parser::parse_statement(sql)?;
        let parse_ms = now_ms() - t0;
        match stmt {
            Statement::Query(q) => self.run_query(&q, opts, parse_ms, false),
            Statement::Explain(q) => self.run_query(&q, opts, parse_ms, true),
            _ => Err("only SELECT and EXPLAIN are allowed here".into()),
        }
    }

    pub fn run_statement(&mut self, stmt: Statement, opts: Options, parse_ms: f64) -> Result<QueryResult, String> {
        match stmt {
            Statement::Query(q) => self.run_query(&q, opts, parse_ms, false),
            Statement::Explain(q) => self.run_query(&q, opts, parse_ms, true),
            Statement::CreateTable { name, columns, if_not_exists } => {
                if self.catalog.tables.contains_key(&name) || self.catalog.views.contains_key(&name) {
                    if if_not_exists {
                        return Ok(QueryResult { affected: Some(0), ..Default::default() });
                    }
                    return Err(format!("Catalog error: table {} already exists", name));
                }
                self.catalog.invalidate(&name);
                self.catalog.tables.insert(name.clone(), Table::empty(&name, columns));
                Ok(QueryResult { affected: Some(0), ..Default::default() })
            }
            Statement::CreateView { name, query } => {
                if self.catalog.tables.contains_key(&name) || self.catalog.views.contains_key(&name) {
                    return Err(format!("Catalog error: {} already exists", name));
                }
                // bind once so errors surface at CREATE time
                planner::Planner::new(&self.catalog).plan_query(&query, None)?;
                self.catalog.views.insert(name, query);
                Ok(QueryResult { affected: Some(0), ..Default::default() })
            }
            Statement::DropTable { name, if_exists } => {
                let gone = self.catalog.tables.remove(&name).is_some() || self.catalog.views.remove(&name).is_some();
                self.catalog.invalidate(&name);
                if !gone && !if_exists {
                    return Err(format!("Catalog error: table {} does not exist", name));
                }
                Ok(QueryResult { affected: Some(0), ..Default::default() })
            }
            Statement::Insert { table, columns, source } => self.insert(&table, columns, source, opts),
            Statement::NoOp => Ok(QueryResult { affected: Some(0), ..Default::default() }),
        }
    }

    fn insert(&mut self, table: &str, columns: Option<Vec<String>>, source: InsertSource, opts: Options) -> Result<QueryResult, String> {
        let t = self.catalog.get(table).ok_or_else(|| format!("Catalog error: table {} does not exist", table))?;
        let tcols = t.columns.clone();
        // target position for each supplied value
        let targets: Vec<usize> = match &columns {
            None => (0..tcols.len()).collect(),
            Some(cs) => cs
                .iter()
                .map(|c| {
                    tcols.iter().position(|(n, _)| n.eq_ignore_ascii_case(c)).ok_or_else(|| format!("Binder error: table {} has no column {}", table, c))
                })
                .collect::<Result<_, _>>()?,
        };
        let raw: Vec<Vec<Value>> = match source {
            InsertSource::Values(rows) => {
                let mut out = Vec::with_capacity(rows.len());
                let mut p = planner::Planner::new(&self.catalog);
                for r in rows {
                    let mut vals = Vec::with_capacity(r.len());
                    for e in &r {
                        let b = p.bind(e, &Schema::default(), None)?;
                        vals.push(exec::eval_const_mode(&b, opts.int_div)?);
                    }
                    out.push(vals);
                }
                out
            }
            InsertSource::Query(q) => {
                let res = self.run_query(&q, Options { record: false, ..opts }, 0.0, false)?;
                (0..res.rows).map(|r| res.columns.iter().map(|c| c.get(r)).collect()).collect()
            }
        };
        let mut rows = Vec::with_capacity(raw.len());
        for r in raw {
            if r.len() != targets.len() {
                return Err(format!("Binder error: table {} has {} target columns but {} values were supplied", table, targets.len(), r.len()));
            }
            let mut full = vec![Value::Null; tcols.len()];
            for (v, &ti) in r.into_iter().zip(&targets) {
                full[ti] = cast_value(&v, tcols[ti].1)?;
            }
            rows.push(full);
        }
        let n = self.catalog.append_rows(table, rows)?;
        Ok(QueryResult { affected: Some(n), ..Default::default() })
    }

    fn run_query(&self, q: &ast::Query, opts: Options, parse_ms: f64, explain: bool) -> Result<QueryResult, String> {
        let t0 = now_ms();
        let plan = planner::Planner::new(&self.catalog).plan_query(q, None)?;
        let t1 = now_ms();
        let mut log = optimizer::OptLog { int_div: opts.int_div, ..Default::default() };
        let optimized = if opts.optimize { optimizer::optimize_logged(plan.clone(), &self.catalog, &mut log)? } else { plan.clone() };
        let t2 = now_ms();
        let schema = optimized.schema();
        if explain {
            let text = optimized.display_tree();
            let mut b = ColumnBuilder::new(DataType::Str, 1);
            b.push(&Value::Str(text));
            return Ok(QueryResult {
                names: vec!["plan".into()],
                types: vec![DataType::Str],
                columns: vec![b.finish()],
                rows: 1,
                plan_before: Some(plan),
                plan_after: Some(optimized),
                opt_log: log.entries,
                parse_ms,
                plan_ms: t1 - t0,
                optimize_ms: t2 - t1,
                ..Default::default()
            });
        }
        let ctx = ExecCtx::new(&self.catalog, opts.int_div, opts.record);
        if let Some(b) = self.pair_budget {
            ctx.pair_budget.set(b);
        }
        let mut pp = PhysicalPlanner::new(&ctx);
        let (mut op, phys) = pp.build(&optimized)?;
        let types: Vec<DataType> = schema.fields.iter().map(|f| f.dtype).collect();
        let out: Batch = collect(op.as_mut(), &ctx, &types)?;
        let t3 = now_ms();
        let stats = ctx.stats.borrow().clone();
        let trace = ctx.trace.borrow().clone();
        Ok(QueryResult {
            names: schema.fields.iter().map(|f| f.name.clone()).collect(),
            types,
            columns: out.columns,
            rows: out.rows,
            plan_before: Some(plan),
            plan_after: Some(optimized),
            physical: Some(phys),
            stats,
            trace,
            opt_log: log.entries,
            parse_ms,
            plan_ms: t1 - t0,
            optimize_ms: t2 - t1,
            exec_ms: t3 - t2,
            affected: None,
        })
    }

    /// Table names and row counts, sorted by name.
    pub fn tables(&self) -> Vec<(String, usize, Vec<(String, DataType)>)> {
        let mut v: Vec<_> = self.catalog.tables.values().map(|t| (t.name.clone(), t.rows, t.columns.clone())).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}
