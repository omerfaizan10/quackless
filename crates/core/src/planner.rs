//! AST to logical plan: name binding, aggregate extraction and subquery decorrelation.
//!
//! Correlated subqueries in WHERE are rewritten into joins when they match one of
//! two shapes (these cover TPC-H):
//!   * `[NOT] EXISTS (SELECT ... WHERE inner_preds AND correlated_preds)` becomes a
//!     SEMI / ANTI join whose join condition is the correlated predicates.
//!   * `expr <op> (SELECT agg(..) FROM .. WHERE inner_col = outer_col AND ..)` becomes
//!     an inner join against the subquery grouped by the correlated columns.
//!     (Not applied to COUNT, whose empty-group value is 0, not NULL.)
//! Anything else stays an expression and is evaluated by re-running the subquery
//! per distinct outer row (slow but correct).

use crate::ast::*;
use crate::catalog::Catalog;
use crate::exec::eval_const;
use crate::expr::*;
use crate::plan::*;
use crate::types::{cast_value, parse_date, DataType, Value};
use std::collections::HashMap;

pub struct Planner<'a> {
    catalog: &'a Catalog,
    ctes: Vec<HashMap<String, Query>>,
    counter: usize,
}

enum OrderRef {
    Item(usize),
    Expr(Expr),
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Planner { catalog, ctes: Vec::new(), counter: 0 }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("__{}{}", prefix, self.counter)
    }

    // ------------------------------------------------------------------ queries

    pub fn plan_query(&mut self, q: &Query, outer: Option<&Schema>) -> Result<LogicalPlan, String> {
        let mut frame = HashMap::new();
        for (name, cq) in &q.ctes {
            frame.insert(name.clone(), cq.clone());
        }
        self.ctes.push(frame);
        let res = self.plan_query_inner(q, outer);
        self.ctes.pop();
        res
    }

    fn plan_query_inner(&mut self, q: &Query, outer: Option<&Schema>) -> Result<LogicalPlan, String> {
        let limit = self.const_usize(q.limit.as_ref())?;
        let offset = self.const_usize(q.offset.as_ref())?.unwrap_or(0);
        match &q.body {
            SetExpr::Select(sel) => self.plan_select(sel, &q.order_by, limit, offset, outer),
            other => {
                let mut plan = self.plan_set_expr(other, outer)?;
                if !q.order_by.is_empty() {
                    let schema = plan.schema();
                    let mut keys = Vec::new();
                    for o in &q.order_by {
                        let e = match &o.expr {
                            AstExpr::Lit(Literal::Number(n)) => {
                                let i: usize = n.parse().map_err(|_| "bad ORDER BY position".to_string())?;
                                let f = schema.fields.get(i.wrapping_sub(1)).ok_or_else(|| {
                                    format!("Binder error: ORDER term out of range - should be between 1 and {}", schema.len())
                                })?;
                                Expr::Column(f.colref())
                            }
                            e => self.bind(e, &schema, outer)?,
                        };
                        keys.push(SortKey { expr: e, asc: o.asc, nulls_first: o.nulls_first.unwrap_or(false) });
                    }
                    plan = LogicalPlan::Sort { input: Box::new(plan), keys };
                }
                if limit.is_some() || offset > 0 {
                    plan = LogicalPlan::Limit { input: Box::new(plan), limit, offset };
                }
                Ok(plan)
            }
        }
    }

    fn const_usize(&mut self, e: Option<&AstExpr>) -> Result<Option<usize>, String> {
        match e {
            None => Ok(None),
            Some(e) => {
                let b = self.bind(e, &Schema::default(), None)?;
                match eval_const(&b)? {
                    Value::Int(i) if i >= 0 => Ok(Some(i as usize)),
                    Value::Null => Ok(None),
                    v => Err(format!("LIMIT/OFFSET must be a non-negative integer, got {}", v)),
                }
            }
        }
    }

    fn plan_set_expr(&mut self, se: &SetExpr, outer: Option<&Schema>) -> Result<LogicalPlan, String> {
        match se {
            SetExpr::Select(s) => self.plan_select(s, &[], None, 0, outer),
            SetExpr::Query(q) => self.plan_query(q, outer),
            SetExpr::SetOp { op, all, left, right } => {
                let l = self.plan_set_expr(left, outer)?;
                let r = self.plan_set_expr(right, outer)?;
                let ls = l.schema();
                let rs = r.schema();
                if ls.len() != rs.len() {
                    return Err("Binder error: set operation children have different column counts".into());
                }
                let fields = ls
                    .fields
                    .iter()
                    .zip(&rs.fields)
                    .enumerate()
                    .map(|(i, (a, b))| Field {
                        rel: Some(format!("__set{}", i)),
                        name: a.name.clone(),
                        dtype: unify(a.dtype, b.dtype),
                    })
                    .collect();
                let schema = Schema::new(fields);
                Ok(match op {
                    SetOp::Union => LogicalPlan::Union { left: Box::new(l), right: Box::new(r), all: *all, schema },
                    SetOp::Except | SetOp::Intersect => LogicalPlan::SetOp {
                        left: Box::new(l),
                        right: Box::new(r),
                        intersect: *op == SetOp::Intersect,
                        schema,
                    },
                })
            }
        }
    }

    // ------------------------------------------------------------------ FROM

    fn plan_from(&mut self, from: &[TableRef], outer: Option<&Schema>) -> Result<LogicalPlan, String> {
        let mut plan: Option<LogicalPlan> = None;
        for t in from {
            let p = self.plan_table_ref(t, outer)?;
            plan = Some(match plan {
                None => p,
                Some(l) => LogicalPlan::Join {
                    left: Box::new(l),
                    right: Box::new(p),
                    kind: JoinType::Cross,
                    on: vec![],
                    filter: None,
                },
            });
        }
        Ok(plan.unwrap_or(LogicalPlan::Values { schema: Schema::default(), rows: vec![vec![]] }))
    }

    fn find_cte(&self, name: &str) -> Option<Query> {
        for f in self.ctes.iter().rev() {
            if let Some(q) = f.get(name) {
                return Some(q.clone());
            }
        }
        None
    }

    fn plan_table_ref(&mut self, t: &TableRef, outer: Option<&Schema>) -> Result<LogicalPlan, String> {
        match t {
            TableRef::Table { name, alias } => {
                let a = alias.clone().unwrap_or_else(|| name.clone());
                if let Some(q) = self.find_cte(name) {
                    // plan the CTE body without the CTE frames above it shadowing differently
                    let p = self.plan_query(&q, None)?;
                    return Ok(make_alias(p, &a, &[]));
                }
                if let Some(q) = self.catalog.views.get(name).cloned() {
                    let p = self.plan_query(&q, None)?;
                    return Ok(make_alias(p, &a, &[]));
                }
                let table = self
                    .catalog
                    .get(name)
                    .ok_or_else(|| format!("Catalog error: Table with name {} does not exist!", name))?;
                let fields = table.columns.iter().map(|(c, dt)| Field::new(Some(&a), c, *dt)).collect();
                Ok(LogicalPlan::Scan { table: name.clone(), alias: a, schema: Schema::new(fields), projection: None })
            }
            TableRef::Subquery { query, alias, columns } => {
                let p = self.plan_query(query, None)?;
                let a = match alias {
                    Some(a) => a.clone(),
                    None => self.fresh("subquery"),
                };
                if columns.len() > p.schema().len() {
                    return Err(format!("Binder error: table {} has fewer columns than aliases", a));
                }
                Ok(make_alias(p, &a, columns))
            }
            TableRef::Join { left, right, kind, on, using } => {
                let l = self.plan_table_ref(left, outer)?;
                let r = self.plan_table_ref(right, outer)?;
                let ls = l.schema();
                let rs = r.schema();
                let joined = ls.join(&rs);
                let mut cond = match on {
                    Some(e) => Some(self.bind(e, &joined, outer)?),
                    None => None,
                };
                for u in using {
                    let li = ls.resolve(None, u)?.ok_or_else(|| format!("Binder error: column {} not found in left side of USING", u))?;
                    let ri = rs.resolve(None, u)?.ok_or_else(|| format!("Binder error: column {} not found in right side of USING", u))?;
                    let eq = Expr::binary(BinOp::Eq, Expr::Column(ls.fields[li].colref()), Expr::Column(rs.fields[ri].colref()));
                    cond = Some(match cond {
                        None => eq,
                        Some(c) => Expr::binary(BinOp::And, c, eq),
                    });
                }
                Ok(match kind {
                    JoinKind::Cross => LogicalPlan::Join { left: Box::new(l), right: Box::new(r), kind: JoinType::Cross, on: vec![], filter: None },
                    JoinKind::Inner => LogicalPlan::Join { left: Box::new(l), right: Box::new(r), kind: JoinType::Inner, on: vec![], filter: cond },
                    JoinKind::Left => LogicalPlan::Join { left: Box::new(l), right: Box::new(r), kind: JoinType::Left, on: vec![], filter: cond },
                    JoinKind::Right => {
                        // a RIGHT JOIN b == b LEFT JOIN a, then restore column order
                        let j = LogicalPlan::Join { left: Box::new(r), right: Box::new(l), kind: JoinType::Left, on: vec![], filter: cond };
                        let exprs: Vec<Expr> = joined.fields.iter().map(|f| Expr::Column(f.colref())).collect();
                        LogicalPlan::Project { input: Box::new(j), exprs, schema: joined }
                    }
                })
            }
        }
    }

    // ------------------------------------------------------------------ SELECT

    fn plan_select(
        &mut self,
        sel: &Select,
        order_by: &[OrderItem],
        limit: Option<usize>,
        offset: usize,
        outer: Option<&Schema>,
    ) -> Result<LogicalPlan, String> {
        let mut plan = self.plan_from(&sel.from, outer)?;
        let from_schema = plan.schema();

        // WHERE, with decorrelation of top-level subquery conjuncts
        if let Some(w) = &sel.selection {
            let mut regular = Vec::new();
            let mut joins: Vec<(JoinType, LogicalPlan, Expr)> = Vec::new();
            let mut post = Vec::new();
            for conj in split_ast_conjuncts(w) {
                match &conj {
                    AstExpr::Exists { query, negated } => {
                        if let Some((right, cond)) = self.decorrelate_exists(query, &from_schema)? {
                            joins.push((if *negated { JoinType::Anti } else { JoinType::Semi }, right, cond));
                            continue;
                        }
                    }
                    AstExpr::Not(inner) => {
                        if let AstExpr::Exists { query, negated } = inner.as_ref() {
                            if let Some((right, cond)) = self.decorrelate_exists(query, &from_schema)? {
                                joins.push((if *negated { JoinType::Semi } else { JoinType::Anti }, right, cond));
                                continue;
                            }
                        }
                    }
                    AstExpr::InSubquery { expr, query, negated: false } => {
                        let sub = self.plan_query(query, Some(&from_schema))?;
                        if !sub.has_outer_refs() && sub.schema().len() == 1 {
                            let sq = self.fresh("sq");
                            let right = make_alias(sub, &sq, &[]);
                            let col = right.schema().fields[0].colref();
                            let l = self.bind(expr, &from_schema, outer)?;
                            joins.push((JoinType::Semi, right, Expr::binary(BinOp::Eq, l, Expr::Column(col))));
                            continue;
                        }
                    }
                    AstExpr::Binary { op, left, right } if op.is_comparison() => {
                        let (sub_side, other, op2) = match (left.as_ref(), right.as_ref()) {
                            (o, AstExpr::Subquery(q)) => (Some(q), o, *op),
                            (AstExpr::Subquery(q), o) => (Some(q), o, op.flip()),
                            _ => (None, left.as_ref(), *op),
                        };
                        if let Some(q) = sub_side {
                            if let Some((rplan, cond, value)) = self.decorrelate_scalar(q, &from_schema)? {
                                let l = self.bind(other, &from_schema, outer)?;
                                joins.push((JoinType::Inner, rplan, cond));
                                post.push(coerce_binary(op2, l, value, &Schema::default()));
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
                regular.push(self.bind(&conj, &from_schema, outer)?);
            }
            if let Some(p) = conjunction(regular) {
                plan = LogicalPlan::Filter { input: Box::new(plan), predicate: p };
            }
            for (kind, right, cond) in joins {
                plan = LogicalPlan::Join { left: Box::new(plan), right: Box::new(right), kind, on: vec![], filter: Some(cond) };
            }
            if let Some(p) = conjunction(post) {
                plan = LogicalPlan::Filter { input: Box::new(plan), predicate: p };
            }
        }
        let schema = plan.schema();

        // select list
        let mut items: Vec<(Expr, String, bool)> = Vec::new(); // (expr, name, explicit alias)
        for it in &sel.projection {
            match it {
                SelectItem::Wildcard => {
                    for f in &from_schema.fields {
                        items.push((Expr::Column(f.colref()), f.name.clone(), false));
                    }
                }
                SelectItem::QualifiedWildcard(q) => {
                    let mut any = false;
                    for f in from_schema.fields.iter().filter(|f| f.rel.as_deref() == Some(q.as_str())) {
                        items.push((Expr::Column(f.colref()), f.name.clone(), false));
                        any = true;
                    }
                    if !any {
                        return Err(format!("Binder error: referenced table \"{}\" not found", q));
                    }
                }
                SelectItem::Expr { expr, alias } => {
                    let e = self.bind(expr, &schema, outer)?;
                    let name = match (alias, &e) {
                        (Some(a), _) => a.clone(),
                        (None, Expr::Column(c)) => c.name.clone(),
                        (None, e) => e.to_string(),
                    };
                    items.push((e, name, alias.is_some()));
                }
            }
        }
        if items.is_empty() {
            return Err("Binder error: SELECT list is empty".into());
        }

        // ORDER BY terms resolved before aggregation (they may contain aggregates)
        let mut orefs = Vec::new();
        for o in order_by {
            let r = match &o.expr {
                AstExpr::Lit(Literal::Number(n)) => {
                    let i: usize = n.parse().map_err(|_| "bad ORDER BY position".to_string())?;
                    if i == 0 || i > items.len() {
                        return Err(format!("Binder error: ORDER term out of range - should be between 1 and {}", items.len()));
                    }
                    OrderRef::Item(i - 1)
                }
                AstExpr::Ident(p) if p.len() == 1 && items.iter().any(|(_, n, a)| *a && n == &p[0]) => {
                    OrderRef::Item(items.iter().position(|(_, n, a)| *a && n == &p[0]).unwrap())
                }
                e => match self.bind(e, &schema, outer) {
                    Ok(b) => match items.iter().position(|(ie, _, _)| *ie == b) {
                        Some(i) => OrderRef::Item(i),
                        None => OrderRef::Expr(b),
                    },
                    Err(err) => {
                        // ORDER BY an output name that is not a source column
                        if let AstExpr::Ident(p) = e {
                            if let Some(i) = items.iter().position(|(_, n, _)| n == p.last().unwrap()) {
                                OrderRef::Item(i)
                            } else {
                                return Err(err);
                            }
                        } else {
                            return Err(err);
                        }
                    }
                },
            };
            orefs.push(r);
        }

        // aggregation
        let having = match &sel.having {
            Some(h) => Some(self.bind(h, &schema, outer)?),
            None => None,
        };
        let has_agg = !sel.group_by.is_empty()
            || items.iter().any(|(e, _, _)| e.contains_agg())
            || having.as_ref().map(|h| h.contains_agg()).unwrap_or(false)
            || orefs.iter().any(|o| matches!(o, OrderRef::Expr(e) if e.contains_agg()));
        let mut having_expr = having;
        if has_agg {
            let mut group = Vec::new();
            for g in &sel.group_by {
                let e = match g {
                    AstExpr::Lit(Literal::Number(n)) => {
                        let i: usize = n.parse().map_err(|_| "bad GROUP BY position".to_string())?;
                        items
                            .get(i.wrapping_sub(1))
                            .map(|x| x.0.clone())
                            .ok_or_else(|| format!("Binder error: GROUP BY term out of range - should be between 1 and {}", items.len()))?
                    }
                    e => match self.bind(e, &schema, outer) {
                        Ok(b) => b,
                        Err(err) => match e {
                            AstExpr::Ident(p) if p.len() == 1 => items
                                .iter()
                                .find(|(_, n, a)| *a && n == &p[0])
                                .map(|x| x.0.clone())
                                .ok_or(err)?,
                            _ => return Err(err),
                        },
                    },
                };
                if e.contains_agg() {
                    return Err("Binder error: GROUP BY clause cannot contain aggregates".into());
                }
                if !group.contains(&e) {
                    group.push(e);
                }
            }
            let mut aggs: Vec<Expr> = Vec::new();
            let mut collect = |e: &Expr| -> Result<(), String> { collect_aggs(e, &mut aggs) };
            for (e, _, _) in &items {
                collect(e)?;
            }
            if let Some(h) = &having_expr {
                collect(h)?;
            }
            for o in &orefs {
                if let OrderRef::Expr(e) = o {
                    collect(e)?;
                }
            }
            let (agg_plan, agg_schema) = build_aggregate(plan, group.clone(), aggs.clone(), &schema, outer)?;
            plan = agg_plan;
            for it in items.iter_mut() {
                it.0 = rewrite_post_agg(it.0.clone(), &group, &aggs, &agg_schema)?;
            }
            if let Some(h) = having_expr.take() {
                let h = rewrite_post_agg(h, &group, &aggs, &agg_schema)?;
                plan = LogicalPlan::Filter { input: Box::new(plan), predicate: h };
            }
            for o in orefs.iter_mut() {
                if let OrderRef::Expr(e) = o {
                    *e = rewrite_post_agg(e.clone(), &group, &aggs, &agg_schema)?;
                }
            }
        } else if having_expr.is_some() {
            return Err("Binder error: HAVING clause without aggregation".into());
        }

        // projection (+ hidden sort columns)
        let n_visible = items.len();
        let mut exprs: Vec<Expr> = items.iter().map(|x| x.0.clone()).collect();
        let mut names: Vec<(Option<String>, String)> = items
            .iter()
            .map(|(e, n, aliased)| match (e, aliased) {
                (Expr::Column(c), false) => (c.rel.clone(), n.clone()),
                _ => (None, n.clone()),
            })
            .collect();
        let mut sort_cols = Vec::new();
        for o in &orefs {
            match o {
                OrderRef::Item(i) => sort_cols.push(*i),
                OrderRef::Expr(e) => {
                    if let Some(i) = exprs.iter().position(|x| x == e) {
                        sort_cols.push(i);
                    } else {
                        if sel.distinct {
                            return Err("Binder error: for SELECT DISTINCT, ORDER BY expressions must appear in select list".into());
                        }
                        exprs.push(e.clone());
                        names.push((Some("__sort".into()), format!("s{}", exprs.len())));
                        sort_cols.push(exprs.len() - 1);
                    }
                }
            }
        }
        // make output column references unique
        let mut seen: Vec<(Option<String>, String)> = Vec::new();
        for (k, n) in names.iter_mut().enumerate() {
            if seen.contains(n) {
                n.0 = Some(format!("__dup{}", k));
            }
            seen.push(n.clone());
        }
        let in_schema = plan.schema();
        let mut fields = Vec::new();
        for (e, (rel, name)) in exprs.iter().zip(&names) {
            fields.push(Field { rel: rel.clone(), name: name.clone(), dtype: expr_type(e, &in_schema, outer)? });
        }
        let proj_schema = Schema::new(fields);
        plan = LogicalPlan::Project { input: Box::new(plan), exprs, schema: proj_schema.clone() };

        if sel.distinct {
            let group: Vec<Expr> = proj_schema.fields.iter().map(|f| Expr::Column(f.colref())).collect();
            plan = LogicalPlan::Aggregate { input: Box::new(plan), group, aggs: vec![], schema: proj_schema.clone() };
        }
        if !sort_cols.is_empty() {
            let keys = sort_cols
                .iter()
                .zip(order_by)
                .map(|(&i, o)| SortKey {
                    expr: Expr::Column(proj_schema.fields[i].colref()),
                    asc: o.asc,
                    nulls_first: o.nulls_first.unwrap_or(false),
                })
                .collect();
            plan = LogicalPlan::Sort { input: Box::new(plan), keys };
        }
        if limit.is_some() || offset > 0 {
            plan = LogicalPlan::Limit { input: Box::new(plan), limit, offset };
        }
        if proj_schema.len() > n_visible {
            let fields: Vec<Field> = proj_schema.fields[..n_visible].to_vec();
            let exprs = fields.iter().map(|f| Expr::Column(f.colref())).collect();
            plan = LogicalPlan::Project { input: Box::new(plan), exprs, schema: Schema::new(fields) };
        }
        Ok(plan)
    }

    // ------------------------------------------------------------------ decorrelation

    /// Simple SELECT body of a subquery, if it has the shape decorrelation needs.
    fn simple_select(q: &Query) -> Option<&Select> {
        if !q.ctes.is_empty() || q.limit.is_some() || q.offset.is_some() {
            return None;
        }
        match &q.body {
            SetExpr::Select(s) if s.group_by.is_empty() && s.having.is_none() => Some(s),
            _ => None,
        }
    }

    /// Plan the subquery's FROM and split its WHERE into (inner-only, correlated) conjuncts.
    fn split_correlated(&mut self, sel: &Select, outer: &Schema) -> Result<Option<(LogicalPlan, Vec<Expr>, Vec<Expr>)>, String> {
        let from = self.plan_from(&sel.from, None)?;
        let s = from.schema();
        let mut inner = Vec::new();
        let mut corr = Vec::new();
        if let Some(w) = &sel.selection {
            for c in split_ast_conjuncts(w) {
                let b = self.bind(&c, &s, Some(outer))?;
                if b.contains_subquery() {
                    return Ok(None);
                }
                if b.contains_outer() {
                    corr.push(b);
                } else {
                    inner.push(b);
                }
            }
        }
        if corr.is_empty() {
            return Ok(None);
        }
        // the correlated predicates must reference uniquely named inner columns
        for c in &corr {
            for col in c.columns() {
                if s.fields.iter().filter(|f| f.name == col.name).count() > 1 {
                    return Ok(None);
                }
            }
        }
        let plan = match conjunction(inner) {
            Some(p) => LogicalPlan::Filter { input: Box::new(from), predicate: p },
            None => from,
        };
        Ok(Some((plan, corr, vec![])))
    }

    fn decorrelate_exists(&mut self, q: &Query, outer: &Schema) -> Result<Option<(LogicalPlan, Expr)>, String> {
        let Some(sel) = Self::simple_select(q) else { return Ok(None) };
        if sel.projection.iter().any(|p| matches!(p, SelectItem::Expr { expr, .. } if ast_has_agg(expr))) {
            return Ok(None);
        }
        let Some((plan, corr, _)) = self.split_correlated(sel, outer)? else { return Ok(None) };
        let sq = self.fresh("sq");
        let right = make_alias(plan, &sq, &[]);
        let cond = conjunction(corr.into_iter().map(|e| requalify_correlated(e, &sq)).collect()).unwrap();
        Ok(Some((right, cond)))
    }

    fn decorrelate_scalar(&mut self, q: &Query, outer: &Schema) -> Result<Option<(LogicalPlan, Expr, Expr)>, String> {
        let Some(sel) = Self::simple_select(q) else { return Ok(None) };
        if sel.distinct || sel.projection.len() != 1 {
            return Ok(None);
        }
        let SelectItem::Expr { expr: pexpr, .. } = &sel.projection[0] else { return Ok(None) };
        if !ast_has_agg(pexpr) {
            return Ok(None);
        }
        let Some((plan, corr, _)) = self.split_correlated(sel, outer)? else { return Ok(None) };
        // every correlated conjunct must be inner_expr = outer_expr
        let mut keys = Vec::new();
        let mut outer_exprs = Vec::new();
        for c in &corr {
            let Expr::Binary { op: BinOp::Eq, left, right } = c else { return Ok(None) };
            let l_outer_only = left.contains_outer() && left.columns().is_empty();
            let r_outer_only = right.contains_outer() && right.columns().is_empty();
            if r_outer_only && !left.contains_outer() {
                keys.push(*left.clone());
                outer_exprs.push(*right.clone());
            } else if l_outer_only && !right.contains_outer() {
                keys.push(*right.clone());
                outer_exprs.push(*left.clone());
            } else {
                return Ok(None);
            }
        }
        let in_schema = plan.schema();
        let value = self.bind(pexpr, &in_schema, None)?;
        let mut aggs = Vec::new();
        collect_aggs(&value, &mut aggs)?;
        if aggs.iter().any(|a| matches!(a, Expr::Agg { func: AggFunc::Count | AggFunc::CountStar, .. })) {
            return Ok(None);
        }
        let (agg_plan, agg_schema) = build_aggregate(plan, keys.clone(), aggs.clone(), &in_schema, None)?;
        let mut exprs = Vec::new();
        let mut fields = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let e = rewrite_post_agg(k.clone(), &keys, &aggs, &agg_schema)?;
            fields.push(Field::new(None, &format!("__k{}", i), expr_type(&e, &agg_schema, None)?));
            exprs.push(e);
        }
        let v = rewrite_post_agg(value, &keys, &aggs, &agg_schema)?;
        fields.push(Field::new(None, "__v", expr_type(&v, &agg_schema, None)?));
        exprs.push(v);
        let proj = LogicalPlan::Project { input: Box::new(agg_plan), exprs, schema: Schema::new(fields) };
        let sq = self.fresh("sq");
        let right = make_alias(proj, &sq, &[]);
        let cond = conjunction(
            outer_exprs
                .into_iter()
                .enumerate()
                .map(|(i, o)| Expr::binary(BinOp::Eq, outer_to_column(o), Expr::col(Some(&sq), &format!("__k{}", i))))
                .collect(),
        )
        .unwrap();
        Ok(Some((right, cond, Expr::col(Some(&sq), "__v"))))
    }

    // ------------------------------------------------------------------ expressions

    pub fn bind(&mut self, e: &AstExpr, schema: &Schema, outer: Option<&Schema>) -> Result<Expr, String> {
        Ok(match e {
            AstExpr::Ident(parts) => {
                let (rel, name) = match parts.len() {
                    1 => (None, parts[0].as_str()),
                    n => (Some(parts[n - 2].as_str()), parts[n - 1].as_str()),
                };
                if let Some(i) = schema.resolve(rel, name)? {
                    Expr::Column(schema.fields[i].colref())
                } else if let Some(i) = match outer {
                    Some(o) => o.resolve(rel, name)?,
                    None => None,
                } {
                    Expr::Outer(outer.unwrap().fields[i].colref())
                } else {
                    return Err(format!("Binder error: Referenced column \"{}\" not found in FROM clause", parts.join(".")));
                }
            }
            AstExpr::Lit(l) => Expr::Lit(match l {
                Literal::Number(n) => {
                    if n.contains('.') || n.contains('e') || n.contains('E') {
                        Value::Float(n.parse().map_err(|_| format!("bad number {}", n))?)
                    } else {
                        match n.parse::<i64>() {
                            Ok(i) => Value::Int(i),
                            Err(_) => Value::Float(n.parse().map_err(|_| format!("bad number {}", n))?),
                        }
                    }
                }
                Literal::Str(s) => Value::Str(s.clone()),
                Literal::Bool(b) => Value::Bool(*b),
                Literal::Null => Value::Null,
            }),
            AstExpr::TypedString { dtype, value } => Expr::Lit(cast_value(&Value::Str(value.clone()), *dtype)?),
            AstExpr::Interval { value, unit } => {
                let n: i32 = value.trim().parse().map_err(|_| format!("bad interval value {}", value))?;
                let (months, days) = match unit.trim_end_matches('s') {
                    "day" => (0, n),
                    "week" => (0, 7 * n),
                    "month" => (n, 0),
                    "year" => (12 * n, 0),
                    u => return Err(format!("unsupported interval unit {}", u)),
                };
                Expr::Lit(Value::Interval { months, days })
            }
            AstExpr::Binary { op, left, right } => {
                let l = self.bind(left, schema, outer)?;
                let r = self.bind(right, schema, outer)?;
                coerce_binary(*op, l, r, schema)
            }
            AstExpr::Neg(x) => match self.bind(x, schema, outer)? {
                Expr::Lit(Value::Int(i)) => Expr::Lit(Value::Int(-i)),
                Expr::Lit(Value::Float(f)) => Expr::Lit(Value::Float(-f)),
                b => Expr::Neg(Box::new(b)),
            },
            AstExpr::Not(x) => Expr::Not(Box::new(self.bind(x, schema, outer)?)),
            AstExpr::Func { name, args, distinct, star } => {
                let func = match name.as_str() {
                    "count" if *star => Some(AggFunc::CountStar),
                    "count" => Some(AggFunc::Count),
                    "sum" => Some(AggFunc::Sum),
                    "avg" | "mean" => Some(AggFunc::Avg),
                    "min" => Some(AggFunc::Min),
                    "max" => Some(AggFunc::Max),
                    _ => None,
                };
                if let Some(f) = func {
                    if f == AggFunc::CountStar {
                        return Ok(Expr::Agg { func: f, arg: None, distinct: false });
                    }
                    if args.len() != 1 {
                        return Err(format!("Binder error: {}() takes exactly one argument", name));
                    }
                    let a = self.bind(&args[0], schema, outer)?;
                    if a.contains_agg() {
                        return Err("Binder error: aggregate function calls cannot be nested".into());
                    }
                    return Ok(Expr::Agg { func: f, arg: Some(Box::new(a)), distinct: *distinct });
                }
                if *star {
                    return Err(format!("Binder error: {}(*) is not supported", name));
                }
                let b: Vec<Expr> = args.iter().map(|a| self.bind(a, schema, outer)).collect::<Result<_, _>>()?;
                let fname = match name.as_str() {
                    "substr" => "substring",
                    "ceiling" => "ceil",
                    "ifnull" => "coalesce",
                    "char_length" | "strlen" => "length",
                    "pow" => "power",
                    "date_part" | "datepart" => {
                        // date_part('year', d)
                        if let Some(Expr::Lit(Value::Str(part))) = b.first() {
                            let f = part.to_ascii_lowercase();
                            return Ok(Expr::Func { name: f, args: b[1..].to_vec() });
                        }
                        return Err("date_part expects a constant part name".into());
                    }
                    n => n,
                };
                let ex = Expr::Func { name: fname.to_string(), args: b };
                expr_type(&ex, schema, outer)?; // validates the function name
                ex
            }
            AstExpr::Case { operand, whens, else_ } => Expr::Case {
                operand: match operand {
                    Some(o) => Some(Box::new(self.bind(o, schema, outer)?)),
                    None => None,
                },
                whens: whens
                    .iter()
                    .map(|(a, b)| Ok((self.bind(a, schema, outer)?, self.bind(b, schema, outer)?)))
                    .collect::<Result<_, String>>()?,
                else_: match else_ {
                    Some(e) => Some(Box::new(self.bind(e, schema, outer)?)),
                    None => None,
                },
            },
            AstExpr::Cast { expr, dtype } => {
                let b = self.bind(expr, schema, outer)?;
                if let Expr::Lit(v) = &b {
                    Expr::Lit(cast_value(v, *dtype)?)
                } else {
                    Expr::Cast { expr: Box::new(b), dtype: *dtype }
                }
            }
            AstExpr::Extract { field, expr } => {
                let f = match field.as_str() {
                    "year" | "month" | "day" | "quarter" | "dayofweek" | "dow" | "dayofyear" | "doy" | "week" => field.clone(),
                    other => return Err(format!("unsupported EXTRACT field {}", other)),
                };
                let f = match f.as_str() {
                    "dow" => "dayofweek".into(),
                    "doy" => "dayofyear".into(),
                    _ => f,
                };
                Expr::Func { name: f, args: vec![self.bind(expr, schema, outer)?] }
            }
            AstExpr::InList { expr, list, negated } => {
                let x = self.bind(expr, schema, outer)?;
                let xt = expr_type(&x, schema, outer).unwrap_or(DataType::Null);
                let mut l = Vec::new();
                for item in list {
                    let b = self.bind(item, schema, outer)?;
                    l.push(coerce_literal(b, xt));
                }
                Expr::InList { expr: Box::new(x), list: l, negated: *negated }
            }
            AstExpr::InSubquery { expr, query, negated } => {
                let x = self.bind(expr, schema, outer)?;
                let p = self.plan_query(query, Some(schema))?;
                if p.schema().len() != 1 {
                    return Err("Binder error: subquery returns more than 1 column".into());
                }
                Expr::InSubquery { expr: Box::new(x), plan: Box::new(p), negated: *negated }
            }
            AstExpr::Exists { query, negated } => {
                let p = self.plan_query(query, Some(schema))?;
                Expr::Exists { plan: Box::new(p), negated: *negated }
            }
            AstExpr::Subquery(q) => {
                let p = self.plan_query(q, Some(schema))?;
                if p.schema().len() != 1 {
                    return Err("Binder error: subquery returns more than 1 column".into());
                }
                Expr::ScalarSubquery(Box::new(p))
            }
            AstExpr::Between { expr, low, high, negated } => {
                let x = self.bind(expr, schema, outer)?;
                let lo = self.bind(low, schema, outer)?;
                let hi = self.bind(high, schema, outer)?;
                let e = Expr::binary(
                    BinOp::And,
                    coerce_binary(BinOp::GtEq, x.clone(), lo, schema),
                    coerce_binary(BinOp::LtEq, x, hi, schema),
                );
                if *negated {
                    Expr::Not(Box::new(e))
                } else {
                    e
                }
            }
            AstExpr::Like { expr, pattern, negated } => Expr::Like {
                expr: Box::new(self.bind(expr, schema, outer)?),
                pattern: Box::new(self.bind(pattern, schema, outer)?),
                negated: *negated,
            },
            AstExpr::IsNull { expr, negated } => Expr::IsNull { expr: Box::new(self.bind(expr, schema, outer)?), negated: *negated },
        })
    }
}

// ---------------------------------------------------------------------- helpers

pub fn make_alias(p: LogicalPlan, alias: &str, columns: &[String]) -> LogicalPlan {
    let s = p.schema();
    let fields = s
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| Field { rel: Some(alias.to_string()), name: columns.get(i).cloned().unwrap_or_else(|| f.name.clone()), dtype: f.dtype })
        .collect();
    LogicalPlan::Alias { input: Box::new(p), alias: alias.to_string(), schema: Schema::new(fields) }
}

fn split_ast_conjuncts(e: &AstExpr) -> Vec<AstExpr> {
    match e {
        AstExpr::Binary { op: BinOp::And, left, right } => {
            let mut v = split_ast_conjuncts(left);
            v.extend(split_ast_conjuncts(right));
            v
        }
        other => vec![other.clone()],
    }
}

fn ast_has_agg(e: &AstExpr) -> bool {
    match e {
        AstExpr::Func { name, args, star, .. } => {
            matches!(name.as_str(), "count" | "sum" | "avg" | "mean" | "min" | "max") && (*star || args.len() == 1)
                || args.iter().any(ast_has_agg)
        }
        AstExpr::Binary { left, right, .. } => ast_has_agg(left) || ast_has_agg(right),
        AstExpr::Neg(x) | AstExpr::Not(x) | AstExpr::Cast { expr: x, .. } => ast_has_agg(x),
        AstExpr::Case { operand, whens, else_ } => {
            operand.as_ref().map(|o| ast_has_agg(o)).unwrap_or(false)
                || whens.iter().any(|(a, b)| ast_has_agg(a) || ast_has_agg(b))
                || else_.as_ref().map(|e| ast_has_agg(e)).unwrap_or(false)
        }
        _ => false,
    }
}

/// String literals compared with dates are read as dates (DuckDB does the same).
fn coerce_literal(e: Expr, target: DataType) -> Expr {
    match (&e, target) {
        (Expr::Lit(Value::Str(s)), DataType::Date) => match parse_date(s) {
            Some(d) => Expr::Lit(Value::Date(d)),
            None => e,
        },
        _ => e,
    }
}

fn coerce_binary(op: BinOp, l: Expr, r: Expr, schema: &Schema) -> Expr {
    if op.is_comparison() {
        let lt = expr_type(&l, schema, None).unwrap_or(DataType::Null);
        let rt = expr_type(&r, schema, None).unwrap_or(DataType::Null);
        let l = coerce_literal(l, rt);
        let r = coerce_literal(r, lt);
        return Expr::binary(op, l, r);
    }
    Expr::binary(op, l, r)
}

fn collect_aggs(e: &Expr, out: &mut Vec<Expr>) -> Result<(), String> {
    let mut err = None;
    e.walk(&mut |x| {
        if let Expr::Agg { arg, .. } = x {
            if let Some(a) = arg {
                if a.contains_subquery() {
                    err = Some("subqueries inside aggregates are not supported".to_string());
                }
            }
            if !out.contains(x) {
                out.push(x.clone());
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn build_aggregate(
    input: LogicalPlan,
    group: Vec<Expr>,
    aggs: Vec<Expr>,
    in_schema: &Schema,
    outer: Option<&Schema>,
) -> Result<(LogicalPlan, Schema), String> {
    let mut fields = Vec::new();
    for (i, g) in group.iter().enumerate() {
        let dt = expr_type(g, in_schema, outer)?;
        fields.push(match g {
            Expr::Column(c) => Field { rel: c.rel.clone(), name: c.name.clone(), dtype: dt },
            _ => Field::new(Some("__grp"), &format!("g{}", i), dt),
        });
    }
    for (i, a) in aggs.iter().enumerate() {
        fields.push(Field::new(Some("__agg"), &format!("a{}", i), expr_type(a, in_schema, outer)?));
    }
    let schema = Schema::new(fields);
    Ok((LogicalPlan::Aggregate { input: Box::new(input), group, aggs, schema: schema.clone() }, schema))
}

fn rewrite_post_agg(e: Expr, group: &[Expr], aggs: &[Expr], schema: &Schema) -> Result<Expr, String> {
    let ng = group.len();
    let out = e.replace(&mut |x| {
        if let Some(i) = group.iter().position(|g| g == x) {
            return Some(Expr::Column(schema.fields[i].colref()));
        }
        if matches!(x, Expr::Agg { .. }) {
            if let Some(j) = aggs.iter().position(|a| a == x) {
                return Some(Expr::Column(schema.fields[ng + j].colref()));
            }
        }
        None
    });
    for c in out.columns() {
        if !schema.contains(&c) {
            return Err(format!(
                "Binder error: column \"{}\" must appear in the GROUP BY clause or must be part of an aggregate function",
                c
            ));
        }
    }
    Ok(out)
}

fn outer_to_column(e: Expr) -> Expr {
    e.replace(&mut |x| match x {
        Expr::Outer(c) => Some(Expr::Column(c.clone())),
        _ => None,
    })
}

/// Correlated predicate rewrite: inner columns move to the alias, outer refs become plain columns.
fn requalify_correlated(e: Expr, alias: &str) -> Expr {
    e.replace(&mut |x| match x {
        Expr::Outer(c) => Some(Expr::Column(c.clone())),
        Expr::Column(c) => Some(Expr::col(Some(alias), &c.name)),
        _ => None,
    })
}
