//! Rule-based optimizer: constant folding, predicate pushdown (with join
//! ordering and equi-key extraction), and projection pruning.

use crate::ast::BinOp;
use crate::catalog::Catalog;
use crate::exec::eval_const_mode;
use crate::expr::*;
use crate::plan::*;
use crate::types::Value;
use std::collections::BTreeSet;

/// Human-readable record of what each rule did, shown in the workbench.
#[derive(Debug, Clone, Default)]
pub struct OptLog {
    pub entries: Vec<(String, String)>,
    /// Fold constants with integer division (SQLite dialect), matching execution.
    pub int_div: bool,
}

impl OptLog {
    fn add(&mut self, rule: &str, msg: String) {
        if self.entries.len() < 200 {
            self.entries.push((rule.to_string(), msg));
        }
    }
}

pub fn optimize(plan: LogicalPlan, catalog: &Catalog, int_div: bool) -> Result<LogicalPlan, String> {
    optimize_logged(plan, catalog, &mut OptLog { int_div, ..Default::default() })
}

pub fn optimize_logged(plan: LogicalPlan, catalog: &Catalog, log: &mut OptLog) -> Result<LogicalPlan, String> {
    let plan = fold_plan(plan, log)?;
    let mut pd = Pushdown { catalog, log };
    let plan = pd.push(plan, vec![])?;
    let plan = fold_plan(plan, &mut OptLog { int_div: log.int_div, ..Default::default() })?;
    let required: BTreeSet<ColRef> = plan.schema().fields.iter().map(|f| f.colref()).collect();
    let plan = prune(plan, &required, log, true)?;
    Ok(plan)
}

// ---------------------------------------------------------------------- constant folding

fn is_foldable(e: &Expr) -> bool {
    !matches!(
        e,
        Expr::Lit(_) | Expr::Column(_) | Expr::Outer(_) | Expr::Agg { .. } | Expr::ScalarSubquery(_) | Expr::Exists { .. } | Expr::InSubquery { .. }
    ) && !e.is_volatile()
        && e.children().iter().all(|c| matches!(c, Expr::Lit(_)))
}

pub fn fold_expr(e: Expr, log: &mut OptLog) -> Result<Expr, String> {
    e.transform(&mut |x| {
        if is_foldable(&x) {
            if let Ok(v) = eval_const_mode(&x, log.int_div) {
                // keep untyped NULL results only when the input had them
                let before = x.to_string();
                let out = Expr::Lit(v);
                if !matches!(x, Expr::Neg(_)) {
                    log.add("constant folding", format!("{}  =>  {}", before, out));
                }
                return Ok(out);
            }
        }
        Ok(match x {
            // NULL in arithmetic makes the result NULL whatever the other side is,
            // so the other side is never evaluated (DuckDB folds this the same way,
            // which matters when that side would raise a division-by-zero error).
            Expr::Binary { op: op @ (BinOp::Plus | BinOp::Minus | BinOp::Mul | BinOp::Div | BinOp::Mod), left, right }
                if (matches!(*left, Expr::Lit(Value::Null)) || matches!(*right, Expr::Lit(Value::Null)))
                    && !left.contains_subquery()
                    && !right.contains_subquery()
                    && !left.contains_agg()
                    && !right.contains_agg() =>
            {
                log.add("constant folding", format!("{}  =>  NULL", Expr::binary(op, *left, *right)));
                Expr::Lit(Value::Null)
            }
            Expr::Binary { op: BinOp::And, left, right } => match (*left, *right) {
                (Expr::Lit(Value::Bool(false)), _) | (_, Expr::Lit(Value::Bool(false))) => Expr::Lit(Value::Bool(false)),
                (Expr::Lit(Value::Bool(true)), y) | (y, Expr::Lit(Value::Bool(true))) => y,
                (l, r) => Expr::binary(BinOp::And, l, r),
            },
            Expr::Binary { op: BinOp::Or, left, right } => match (*left, *right) {
                (Expr::Lit(Value::Bool(true)), _) | (_, Expr::Lit(Value::Bool(true))) => Expr::Lit(Value::Bool(true)),
                (Expr::Lit(Value::Bool(false)), y) | (y, Expr::Lit(Value::Bool(false))) => y,
                (l, r) => Expr::binary(BinOp::Or, l, r),
            },
            other => other,
        })
    })
}

fn fold_plan(plan: LogicalPlan, log: &mut OptLog) -> Result<LogicalPlan, String> {
    plan.transform_up(&mut |node| {
        let node = node.map_exprs(&mut |e| fold_expr(e, log))?;
        Ok(match node {
            LogicalPlan::Filter { input, predicate: Expr::Lit(Value::Bool(true)) } => *input,
            other => other,
        })
    })
}

// ---------------------------------------------------------------------- predicate pushdown

struct Pushdown<'a, 'l> {
    catalog: &'a Catalog,
    log: &'l mut OptLog,
}

fn covers(schema: &Schema, e: &Expr) -> bool {
    e.columns().iter().all(|c| schema.contains(c))
}

fn pushable(e: &Expr) -> bool {
    !e.contains_subquery() && !e.is_volatile() && !e.contains_agg()
}

fn wrap_filter(plan: LogicalPlan, preds: Vec<Expr>) -> LogicalPlan {
    match conjunction(preds) {
        Some(p) => LogicalPlan::Filter { input: Box::new(plan), predicate: p },
        None => plan,
    }
}

/// `(a AND x) OR (a AND y)`  =>  `a AND (x OR y)`
fn factor_or(e: Expr) -> Vec<Expr> {
    if !matches!(e, Expr::Binary { op: BinOp::Or, .. }) {
        return vec![e];
    }
    let disj: Vec<Vec<Expr>> = split_disjuncts(&e).into_iter().map(split_conjuncts).collect();
    let common: Vec<Expr> = disj[0].iter().filter(|c| disj.iter().all(|d| d.contains(c))).cloned().collect();
    if common.is_empty() {
        return vec![e];
    }
    let rest: Vec<Expr> = disj
        .into_iter()
        .map(|d| conjunction(d.into_iter().filter(|c| !common.contains(c)).collect()).unwrap_or(Expr::Lit(Value::Bool(true))))
        .collect();
    let mut out = common;
    if !rest.iter().any(|r| *r == Expr::Lit(Value::Bool(true))) {
        out.push(disjunction(rest).unwrap());
    }
    out
}

/// Split a join predicate into equi keys (left expr, right expr) and the rest.
fn extract_keys(preds: Vec<Expr>, ls: &Schema, rs: &Schema) -> (Vec<(Expr, Expr)>, Vec<Expr>) {
    let mut keys = Vec::new();
    let mut rest = Vec::new();
    for p in preds {
        if let Expr::Binary { op: BinOp::Eq, left, right } = &p {
            let (lc, rc) = (left.columns(), right.columns());
            if !lc.is_empty() && !rc.is_empty() && pushable(&p) {
                if covers(ls, left) && covers(rs, right) {
                    keys.push((*left.clone(), *right.clone()));
                    continue;
                }
                if covers(rs, left) && covers(ls, right) {
                    keys.push((*right.clone(), *left.clone()));
                    continue;
                }
            }
        }
        rest.push(p);
    }
    (keys, rest)
}

fn selectivity(e: &Expr) -> f64 {
    match e {
        Expr::Binary { op: BinOp::Eq, .. } => 0.1,
        Expr::Binary { op: BinOp::Or, .. } => 0.4,
        Expr::Binary { op, .. } if op.is_comparison() => 0.35,
        Expr::Binary { op: BinOp::And, left, right } => selectivity(left) * selectivity(right),
        Expr::Like { negated: false, .. } => 0.2,
        Expr::InList { negated: false, .. } => 0.2,
        _ => 0.6,
    }
}

pub fn estimate(plan: &LogicalPlan, catalog: &Catalog) -> f64 {
    match plan {
        LogicalPlan::Scan { table, .. } => catalog.get(table).map(|t| t.rows as f64).unwrap_or(1000.0),
        LogicalPlan::Values { rows, .. } => rows.len() as f64,
        LogicalPlan::Filter { input, predicate } => {
            let s: f64 = split_conjuncts(predicate.clone()).iter().map(selectivity).product();
            (estimate(input, catalog) * s).max(1.0)
        }
        LogicalPlan::Join { left, right, kind, on, .. } => {
            let (l, r) = (estimate(left, catalog), estimate(right, catalog));
            match kind {
                JoinType::Semi | JoinType::Anti => l * 0.5,
                JoinType::Left => l.max(r),
                _ if on.is_empty() => l * r,
                _ => l.max(r),
            }
        }
        LogicalPlan::Aggregate { input, group, .. } => {
            if group.is_empty() { 1.0 } else { (estimate(input, catalog) * 0.1).max(1.0) }
        }
        LogicalPlan::Limit { input, limit, .. } => {
            let e = estimate(input, catalog);
            limit.map(|l| e.min(l as f64)).unwrap_or(e)
        }
        LogicalPlan::Union { left, right, .. } | LogicalPlan::SetOp { left, right, .. } => estimate(left, catalog) + estimate(right, catalog),
        LogicalPlan::Project { input, .. } | LogicalPlan::Sort { input, .. } | LogicalPlan::Alias { input, .. } => estimate(input, catalog),
    }
}

/// Distinct count of a base-table column reached through filters.
fn base_ndv(p: &LogicalPlan, c: &ColRef, catalog: &Catalog) -> Option<f64> {
    match p {
        LogicalPlan::Filter { input, .. } => base_ndv(input, c, catalog),
        LogicalPlan::Scan { table, schema, .. } => {
            let i = schema.index_of(c)?;
            catalog.ndv(table, i)
        }
        _ => None,
    }
}

/// Distinct count of a join key: a plain column uses the statistics, anything else
/// is assumed unique within its input.
fn key_ndv(e: &Expr, ndv: &std::collections::HashMap<ColRef, f64>, rows: f64) -> f64 {
    match e {
        Expr::Column(c) => ndv.get(c).copied().unwrap_or(rows).min(rows).max(1.0),
        _ => rows.max(1.0),
    }
}

fn leaf_name(p: &LogicalPlan) -> String {
    match p {
        LogicalPlan::Scan { alias, .. } => alias.clone(),
        LogicalPlan::Alias { alias, .. } => alias.trim_start_matches('_').to_string(),
        LogicalPlan::Filter { input, .. } => leaf_name(input),
        other => other.node_label().0.to_lowercase(),
    }
}

impl<'a, 'l> Pushdown<'a, 'l> {
    fn push(&mut self, plan: LogicalPlan, preds: Vec<Expr>) -> Result<LogicalPlan, String> {
        match plan {
            LogicalPlan::Filter { input, predicate } => {
                let mut all = preds;
                all.extend(split_conjuncts(predicate));
                self.push(*input, all)
            }
            LogicalPlan::Join { kind: JoinType::Inner | JoinType::Cross, .. } => self.join_region(plan, preds),
            LogicalPlan::Join { left, right, kind, on, filter } => {
                let ls = left.schema();
                let rs = right.schema();
                let mut up = Vec::new();
                let mut to_left = Vec::new();
                for p in preds {
                    if pushable(&p) && covers(&ls, &p) {
                        to_left.push(p);
                    } else {
                        up.push(p);
                    }
                }
                let own = filter.map(split_conjuncts).unwrap_or_default();
                let (mut keys, rest) = extract_keys(own, &ls, &rs);
                let mut keys0 = on;
                keys0.append(&mut keys);
                let mut residual = Vec::new();
                let mut to_right = Vec::new();
                for p in rest {
                    if pushable(&p) && covers(&rs, &p) && !p.columns().is_empty() {
                        to_right.push(p);
                    } else {
                        residual.push(p);
                    }
                }
                if !keys0.is_empty() {
                    self.log.add(
                        "join keys",
                        format!("{} JOIN on {}", kind.name(), keys0.iter().map(|(a, b)| format!("{} = {}", a, b)).collect::<Vec<_>>().join(" AND ")),
                    );
                }
                let l = self.push(*left, to_left)?;
                let r = self.push(*right, to_right)?;
                let j = LogicalPlan::Join { left: Box::new(l), right: Box::new(r), kind, on: keys0, filter: conjunction(residual) };
                Ok(wrap_filter(j, up))
            }
            LogicalPlan::Project { input, exprs, schema } => {
                let mut down = Vec::new();
                let mut up = Vec::new();
                for p in preds {
                    let ok = pushable(&p)
                        && covers(&schema, &p)
                        && p.columns().iter().all(|c| {
                            let e = &exprs[schema.index_of(c).unwrap()];
                            pushable(e) && !e.contains_subquery()
                        });
                    if ok {
                        down.push(p.replace(&mut |x| match x {
                            Expr::Column(c) => schema.index_of(c).map(|i| exprs[i].clone()),
                            _ => None,
                        }));
                    } else {
                        up.push(p);
                    }
                }
                let child = self.push(*input, down)?;
                Ok(wrap_filter(LogicalPlan::Project { input: Box::new(child), exprs, schema }, up))
            }
            LogicalPlan::Alias { input, alias, schema } => {
                let inner = input.schema();
                let mut down = Vec::new();
                let mut up = Vec::new();
                for p in preds {
                    if pushable(&p) && covers(&schema, &p) {
                        down.push(p.replace(&mut |x| match x {
                            Expr::Column(c) => schema.index_of(c).map(|i| Expr::Column(inner.fields[i].colref())),
                            _ => None,
                        }));
                    } else {
                        up.push(p);
                    }
                }
                if !down.is_empty() {
                    self.log.add("predicate pushdown", format!("{} predicate(s) into derived table {}", down.len(), alias.trim_start_matches('_')));
                }
                let child = self.push(*input, down)?;
                Ok(wrap_filter(LogicalPlan::Alias { input: Box::new(child), alias, schema }, up))
            }
            LogicalPlan::Aggregate { input, group, aggs, schema } => {
                let ng = group.len();
                let mut down = Vec::new();
                let mut up = Vec::new();
                for p in preds {
                    let ok = pushable(&p)
                        && !p.columns().is_empty()
                        && p.columns().iter().all(|c| schema.index_of(c).map(|i| i < ng).unwrap_or(false));
                    if ok {
                        down.push(p.replace(&mut |x| match x {
                            Expr::Column(c) => schema.index_of(c).filter(|i| *i < ng).map(|i| group[i].clone()),
                            _ => None,
                        }));
                    } else {
                        up.push(p);
                    }
                }
                let child = self.push(*input, down)?;
                Ok(wrap_filter(LogicalPlan::Aggregate { input: Box::new(child), group, aggs, schema }, up))
            }
            LogicalPlan::Sort { input, keys } => {
                let child = self.push(*input, preds)?;
                Ok(LogicalPlan::Sort { input: Box::new(child), keys })
            }
            LogicalPlan::Scan { .. } | LogicalPlan::Values { .. } => {
                if !preds.is_empty() {
                    if let LogicalPlan::Scan { alias, .. } = &plan {
                        self.log.add(
                            "predicate pushdown",
                            format!("{} => scan of {}", preds.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(" AND "), alias),
                        );
                    }
                }
                Ok(wrap_filter(plan, preds))
            }
            other => {
                // Limit, Union, SetOp: optimize below, keep predicates here
                let kids: Vec<LogicalPlan> = other.children().into_iter().cloned().collect();
                let mut nk = Vec::new();
                for k in kids {
                    nk.push(self.push(k, vec![])?);
                }
                Ok(wrap_filter(other.with_children(nk), preds))
            }
        }
    }

    fn flatten(&mut self, plan: LogicalPlan, leaves: &mut Vec<LogicalPlan>, preds: &mut Vec<Expr>) {
        match plan {
            LogicalPlan::Join { left, right, kind: JoinType::Inner | JoinType::Cross, on, filter } => {
                for (a, b) in on {
                    preds.push(Expr::binary(BinOp::Eq, a, b));
                }
                if let Some(f) = filter {
                    preds.extend(split_conjuncts(f));
                }
                self.flatten(*left, leaves, preds);
                self.flatten(*right, leaves, preds);
            }
            LogicalPlan::Filter { input, predicate } if matches!(*input, LogicalPlan::Join { kind: JoinType::Inner | JoinType::Cross, .. }) => {
                preds.extend(split_conjuncts(predicate));
                self.flatten(*input, leaves, preds);
            }
            other => leaves.push(other),
        }
    }

    /// Flatten a tree of inner joins / cross products, place every predicate as
    /// low as possible, then build a left-deep join tree greedily: start from the
    /// largest input (the probe side) and repeatedly add the smallest input that
    /// is connected to what has been joined so far (it becomes the build side).
    fn join_region(&mut self, plan: LogicalPlan, preds: Vec<Expr>) -> Result<LogicalPlan, String> {
        let mut leaves = Vec::new();
        let mut pool = preds;
        self.flatten(plan, &mut leaves, &mut pool);
        let schemas: Vec<Schema> = leaves.iter().map(|l| l.schema()).collect();

        // OR factoring, then implied single-input filters from ORs
        let mut factored = Vec::new();
        for p in pool {
            let parts = factor_or(p.clone());
            if parts.len() > 1 {
                self.log.add("or factoring", format!("pulled {} common conjunct(s) out of an OR", parts.len() - 1));
            }
            factored.extend(parts);
        }
        let mut derived = Vec::new();
        for p in &factored {
            if !matches!(p, Expr::Binary { op: BinOp::Or, .. }) || !pushable(p) {
                continue;
            }
            let disj: Vec<Vec<Expr>> = split_disjuncts(p).into_iter().map(split_conjuncts).collect();
            if schemas.iter().filter(|s| p.columns().iter().any(|c| s.contains(c))).count() < 2 {
                continue;
            }
            for s in schemas.iter() {
                let mut parts = Vec::new();
                for d in &disj {
                    let mine: Vec<Expr> = d.iter().filter(|c| !c.columns().is_empty() && covers(s, c)).cloned().collect();
                    if mine.is_empty() {
                        parts.clear();
                        break;
                    }
                    parts.push(conjunction(mine).unwrap());
                }
                if !parts.is_empty() {
                    let d = disjunction(parts).unwrap();
                    self.log.add("implied filter", format!("derived {} from an OR", d));
                    derived.push(d);
                }
            }
        }
        factored.extend(derived);

        let mut assigned: Vec<Vec<Expr>> = vec![Vec::new(); leaves.len()];
        let mut join_preds = Vec::new();
        let mut top = Vec::new();
        for p in factored {
            if !pushable(&p) {
                top.push(p);
                continue;
            }
            let cols = p.columns();
            if cols.is_empty() {
                top.push(p);
                continue;
            }
            let hit: Vec<usize> = (0..leaves.len()).filter(|&i| cols.iter().any(|c| schemas[i].contains(c))).collect();
            if hit.len() == 1 && covers(&schemas[hit[0]], &p) {
                assigned[hit[0]].push(p);
            } else if hit.is_empty() {
                top.push(p);
            } else {
                join_preds.push(p);
            }
        }
        let mut items: Vec<Option<LogicalPlan>> = Vec::new();
        for (leaf, ps) in leaves.into_iter().zip(assigned) {
            items.push(Some(self.push(leaf, ps)?));
        }
        let ests: Vec<f64> = items.iter().map(|p| estimate(p.as_ref().unwrap(), self.catalog)).collect();

        let n = items.len();
        // distinct counts for every column a join predicate mentions, capped by the
        // (filtered) size of its input
        let mut ndv: std::collections::HashMap<ColRef, f64> = std::collections::HashMap::new();
        for p in &join_preds {
            for c in p.columns() {
                if ndv.contains_key(&c) {
                    continue;
                }
                if let Some(i) = (0..n).find(|&i| schemas[i].contains(&c)) {
                    let base = base_ndv(items[i].as_ref().unwrap(), &c, self.catalog).unwrap_or(ests[i]);
                    ndv.insert(c, base.min(ests[i]).max(1.0));
                }
            }
        }
        let start = (0..n).max_by(|&a, &b| ests[a].partial_cmp(&ests[b]).unwrap()).unwrap();
        let mut current = items[start].take().unwrap();
        let mut cur_schema = schemas[start].clone();
        let mut cur_est = ests[start];
        let mut joined = vec![false; n];
        joined[start] = true;
        let mut order = vec![leaf_name(&current)];
        for _ in 1..n {
            // greedy: join next whichever connected input gives the smallest estimated result
            let mut best: Option<(usize, f64)> = None;
            for i in (0..n).filter(|&i| !joined[i]) {
                let both = cur_schema.join(&schemas[i]);
                let applicable: Vec<Expr> = join_preds
                    .iter()
                    .filter(|p| covers(&both, p) && p.columns().iter().any(|c| schemas[i].contains(c)) && p.columns().iter().any(|c| cur_schema.contains(c)))
                    .cloned()
                    .collect();
                let out = if applicable.is_empty() {
                    f64::INFINITY // a cross product: only when nothing else is connected
                } else {
                    let (keys, rest) = extract_keys(applicable, &cur_schema, &schemas[i]);
                    let mut sel = 1.0f64;
                    for (a, b) in &keys {
                        let na = key_ndv(a, &ndv, cur_est);
                        let nb = key_ndv(b, &ndv, ests[i]);
                        sel = sel.min(1.0 / na.max(nb));
                    }
                    if keys.is_empty() {
                        sel = 0.5;
                    }
                    (cur_est * ests[i] * sel * 0.9f64.powi(rest.len() as i32)).max(1.0)
                };
                let better = match best {
                    None => true,
                    Some((j, b)) => out < b || (out == b && ests[i] < ests[j]),
                };
                if better {
                    best = Some((i, out));
                }
            }
            let (next, out) = best.unwrap();
            let right = items[next].take().unwrap();
            let both = cur_schema.join(&schemas[next]);
            let (applicable, remaining): (Vec<Expr>, Vec<Expr>) = join_preds.into_iter().partition(|p| covers(&both, p));
            join_preds = remaining;
            let (keys, rest) = extract_keys(applicable, &cur_schema, &schemas[next]);
            order.push(leaf_name(&right));
            let kind = if keys.is_empty() && rest.is_empty() { JoinType::Cross } else { JoinType::Inner };
            current = LogicalPlan::Join { left: Box::new(current), right: Box::new(right), kind, on: keys, filter: conjunction(rest) };
            cur_schema = both;
            cur_est = if out.is_finite() { out } else { cur_est * ests[next] };
            joined[next] = true;
        }
        if n > 1 {
            self.log.add("join order", format!("probe {} then build {}", order[0], order[1..].join(", ")));
        }
        top.extend(join_preds);
        Ok(wrap_filter(current, top))
    }
}

// ---------------------------------------------------------------------- projection pruning

fn prune(plan: LogicalPlan, req: &BTreeSet<ColRef>, log: &mut OptLog, is_root: bool) -> Result<LogicalPlan, String> {
    Ok(match plan {
        LogicalPlan::Scan { table, alias, schema, projection } => {
            let mut idx: Vec<usize> = (0..schema.len()).filter(|&i| req.contains(&schema.fields[i].colref())).collect();
            if let Some(p) = &projection {
                idx.retain(|i| p.contains(i));
            }
            if idx.is_empty() {
                idx.push(0);
            }
            if idx.len() < schema.len() {
                log.add("projection pruning", format!("scan {} reads {} of {} columns", alias, idx.len(), schema.len()));
            }
            LogicalPlan::Scan { table, alias, schema, projection: Some(idx) }
        }
        LogicalPlan::Values { .. } => plan,
        LogicalPlan::Filter { input, predicate } => {
            let mut r = req.clone();
            r.extend(predicate.columns());
            LogicalPlan::Filter { input: Box::new(prune(*input, &r, log, false)?), predicate }
        }
        LogicalPlan::Project { input, exprs, schema } => {
            let mut keep: Vec<usize> = (0..exprs.len()).filter(|&i| is_root || req.contains(&schema.fields[i].colref())).collect();
            if keep.is_empty() {
                keep.push(0);
            }
            let mut r = BTreeSet::new();
            for &i in &keep {
                r.extend(exprs[i].columns());
            }
            let child = prune(*input, &r, log, false)?;
            LogicalPlan::Project {
                input: Box::new(child),
                exprs: keep.iter().map(|&i| exprs[i].clone()).collect(),
                schema: Schema::new(keep.iter().map(|&i| schema.fields[i].clone()).collect()),
            }
        }
        LogicalPlan::Aggregate { input, group, aggs, schema } => {
            let ng = group.len();
            let keep: Vec<usize> = (0..aggs.len()).filter(|&j| is_root || req.contains(&schema.fields[ng + j].colref())).collect();
            let mut r = BTreeSet::new();
            for g in &group {
                r.extend(g.columns());
            }
            for &j in &keep {
                r.extend(aggs[j].columns());
            }
            let child = prune(*input, &r, log, false)?;
            let mut fields: Vec<Field> = schema.fields[..ng].to_vec();
            fields.extend(keep.iter().map(|&j| schema.fields[ng + j].clone()));
            LogicalPlan::Aggregate {
                input: Box::new(child),
                group,
                aggs: keep.iter().map(|&j| aggs[j].clone()).collect(),
                schema: Schema::new(fields),
            }
        }
        LogicalPlan::Join { left, right, kind, on, filter } => {
            let mut r = req.clone();
            for (a, b) in &on {
                r.extend(a.columns());
                r.extend(b.columns());
            }
            if let Some(f) = &filter {
                r.extend(f.columns());
            }
            let ls = left.schema();
            let rs = right.schema();
            let lr: BTreeSet<ColRef> = r.iter().filter(|c| ls.contains(c)).cloned().collect();
            let rr: BTreeSet<ColRef> = r.iter().filter(|c| rs.contains(c)).cloned().collect();
            LogicalPlan::Join {
                left: Box::new(prune(*left, &lr, log, false)?),
                right: Box::new(prune(*right, &rr, log, false)?),
                kind,
                on,
                filter,
            }
        }
        LogicalPlan::Sort { input, keys } => {
            let mut r = req.clone();
            for k in &keys {
                r.extend(k.expr.columns());
            }
            LogicalPlan::Sort { input: Box::new(prune(*input, &r, log, is_root)?), keys }
        }
        LogicalPlan::Limit { input, limit, offset } => {
            LogicalPlan::Limit { input: Box::new(prune(*input, req, log, is_root)?), limit, offset }
        }
        LogicalPlan::Alias { input, alias, schema } => {
            let old = input.schema();
            let r: BTreeSet<ColRef> = (0..schema.len())
                .filter(|&i| is_root || req.contains(&schema.fields[i].colref()))
                .map(|i| old.fields[i].colref())
                .collect();
            let child = prune(*input, &r, log, false)?;
            let new = child.schema();
            let mut fields = Vec::new();
            for f in &new.fields {
                let i = old.index_of(&f.colref()).ok_or("internal: alias pruning lost a column")?;
                fields.push(schema.fields[i].clone());
            }
            LogicalPlan::Alias { input: Box::new(child), alias, schema: Schema::new(fields) }
        }
        LogicalPlan::Union { left, right, all, schema } => {
            let lr = left.schema().fields.iter().map(|f| f.colref()).collect();
            let rr = right.schema().fields.iter().map(|f| f.colref()).collect();
            LogicalPlan::Union {
                left: Box::new(prune(*left, &lr, log, true)?),
                right: Box::new(prune(*right, &rr, log, true)?),
                all,
                schema,
            }
        }
        LogicalPlan::SetOp { left, right, intersect, schema } => {
            let lr = left.schema().fields.iter().map(|f| f.colref()).collect();
            let rr = right.schema().fields.iter().map(|f| f.colref()).collect();
            LogicalPlan::SetOp {
                left: Box::new(prune(*left, &lr, log, true)?),
                right: Box::new(prune(*right, &rr, log, true)?),
                intersect,
                schema,
            }
        }
    })
}
