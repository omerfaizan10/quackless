//! Hand-written recursive descent parser.
//!
//! Expression precedence, lowest to highest:
//! OR, AND, NOT, comparison / IS / IN / BETWEEN / LIKE, ||, + -, * / %, unary -, primary.

use crate::ast::*;
use crate::lexer::{tokenize, Tok, Token};
use crate::types::DataType;

pub struct Parser {
    toks: Vec<Token>,
    i: usize,
}

const RESERVED: &[&str] = &[
    "select", "from", "where", "group", "order", "having", "limit", "offset", "union", "except", "intersect", "join",
    "inner", "left", "right", "cross", "full", "outer", "natural", "on", "using", "and", "or", "not", "as", "when",
    "then", "else", "end", "asc", "desc", "nulls", "is", "in", "between", "like", "values", "case", "exists", "by",
    "with", "distinct", "all", "set",
];

pub fn parse_statements(sql: &str) -> Result<Vec<Statement>, String> {
    let mut p = Parser { toks: tokenize(sql)?, i: 0 };
    let mut out = Vec::new();
    loop {
        while p.eat_sym(";") {}
        if p.peek() == &Tok::Eof {
            break;
        }
        out.push(p.statement()?);
        if !p.eat_sym(";") && p.peek() != &Tok::Eof {
            return Err(p.err("expected end of statement"));
        }
    }
    Ok(out)
}

pub fn parse_statement(sql: &str) -> Result<Statement, String> {
    let mut v = parse_statements(sql)?;
    match v.len() {
        0 => Err("empty statement".into()),
        1 => Ok(v.remove(0)),
        _ => Err("expected a single statement".into()),
    }
}

pub fn parse_type_name(name: &str) -> Result<DataType, String> {
    Ok(match name {
        "int" | "integer" | "bigint" | "smallint" | "tinyint" | "int4" | "int8" | "int2" | "hugeint" | "long" => {
            DataType::Int
        }
        "decimal" | "numeric" | "double" | "float" | "real" | "float8" | "float4" => DataType::Float,
        "varchar" | "text" | "char" | "string" | "character" | "bpchar" => DataType::Str,
        "date" => DataType::Date,
        "boolean" | "bool" => DataType::Bool,
        "interval" => DataType::Interval,
        other => return Err(format!("unsupported type {}", other.to_uppercase())),
    })
}

/// `l IS [NOT] DISTINCT FROM r`, desugared to a NULL-safe CASE:
/// both NULL -> equal, one NULL -> different, otherwise l = r.
fn distinct_from(l: AstExpr, r: AstExpr, not: bool) -> AstExpr {
    let is_null = |e: &AstExpr| AstExpr::IsNull { expr: Box::new(e.clone()), negated: false };
    let both = AstExpr::Binary { op: BinOp::And, left: Box::new(is_null(&l)), right: Box::new(is_null(&r)) };
    let either = AstExpr::Binary { op: BinOp::Or, left: Box::new(is_null(&l)), right: Box::new(is_null(&r)) };
    let eq = AstExpr::Binary { op: if not { BinOp::Eq } else { BinOp::NotEq }, left: Box::new(l), right: Box::new(r) };
    AstExpr::Case {
        operand: None,
        whens: vec![(both, AstExpr::Lit(Literal::Bool(not))), (either, AstExpr::Lit(Literal::Bool(!not)))],
        else_: Some(Box::new(eq)),
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }
    fn peek_at(&self, k: usize) -> &Tok {
        &self.toks[(self.i + k).min(self.toks.len() - 1)].tok
    }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.i].tok.clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn err(&self, msg: &str) -> String {
        let near = match self.peek() {
            Tok::Word { value, .. } => value.clone(),
            Tok::Number(n) => n.clone(),
            Tok::Str(s) => format!("'{}'", s),
            Tok::Sym(s) => s.to_string(),
            Tok::Eof => "end of input".into(),
        };
        format!("Parser error: {} near \"{}\" (position {})", msg, near, self.toks[self.i].pos)
    }
    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Word { value, quoted: false } if value == kw)
    }
    fn is_kw_at(&self, k: usize, kw: &str) -> bool {
        matches!(self.peek_at(k), Tok::Word { value, quoted: false } if value == kw)
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, kw: &str) -> Result<(), String> {
        if self.eat_kw(kw) {
            Ok(())
        } else {
            Err(self.err(&format!("expected {}", kw.to_uppercase())))
        }
    }
    fn eat_sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Tok::Sym(x) if *x == s) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect_sym(&mut self, s: &str) -> Result<(), String> {
        if self.eat_sym(s) {
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", s)))
        }
    }
    fn ident(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            Tok::Word { value, quoted } => {
                if !quoted && RESERVED.contains(&value.as_str()) {
                    return Err(self.err("expected identifier"));
                }
                self.next();
                Ok(value)
            }
            _ => Err(self.err("expected identifier")),
        }
    }
    fn is_ident_start(&self) -> bool {
        matches!(self.peek(), Tok::Word { value, quoted } if *quoted || !RESERVED.contains(&value.as_str()))
    }

    // ------------------------------------------------------------------ statements

    fn statement(&mut self) -> Result<Statement, String> {
        if self.is_kw("select") || self.is_kw("with") || matches!(self.peek(), Tok::Sym("(")) {
            return Ok(Statement::Query(self.query()?));
        }
        if self.eat_kw("explain") {
            return Ok(Statement::Explain(self.query()?));
        }
        if self.eat_kw("create") {
            if self.eat_kw("table") {
                let if_not_exists = if self.eat_kw("if") {
                    self.expect_kw("not")?;
                    self.expect_kw("exists")?;
                    true
                } else {
                    false
                };
                let name = self.ident()?;
                self.expect_sym("(")?;
                let mut columns = Vec::new();
                loop {
                    if self.is_kw("primary") || self.is_kw("unique") || self.is_kw("foreign") {
                        self.skip_parens_until_comma();
                    } else {
                        let col = self.ident()?;
                        let tname = self.ident_any()?;
                        let dt = parse_type_name(&tname)?;
                        if self.eat_sym("(") {
                            while !self.eat_sym(")") {
                                self.next();
                            }
                        }
                        // constraints: NOT NULL, NULL, PRIMARY KEY, DEFAULT x ...
                        while !matches!(self.peek(), Tok::Sym(",") | Tok::Sym(")") | Tok::Eof) {
                            self.next();
                        }
                        columns.push((col, dt));
                    }
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym(")")?;
                return Ok(Statement::CreateTable { name, columns, if_not_exists });
            }
            if self.eat_kw("view") {
                let name = self.ident()?;
                self.expect_kw("as")?;
                return Ok(Statement::CreateView { name, query: self.query()? });
            }
            // CREATE [UNIQUE] INDEX ... : accepted, ignored.
            self.skip_to_end();
            return Ok(Statement::NoOp);
        }
        if self.eat_kw("drop") {
            if self.eat_kw("table") || self.eat_kw("view") {
                let if_exists = if self.eat_kw("if") {
                    self.expect_kw("exists")?;
                    true
                } else {
                    false
                };
                let name = self.ident()?;
                return Ok(Statement::DropTable { name, if_exists });
            }
            self.skip_to_end();
            return Ok(Statement::NoOp);
        }
        if self.eat_kw("insert") {
            self.expect_kw("into")?;
            let table = self.ident()?;
            let columns = if matches!(self.peek(), Tok::Sym("(")) && !self.is_kw_at(1, "select") {
                self.next();
                let mut cols = vec![self.ident()?];
                while self.eat_sym(",") {
                    cols.push(self.ident()?);
                }
                self.expect_sym(")")?;
                Some(cols)
            } else {
                None
            };
            if self.eat_kw("values") {
                let mut rows = Vec::new();
                loop {
                    self.expect_sym("(")?;
                    let mut row = vec![self.expr()?];
                    while self.eat_sym(",") {
                        row.push(self.expr()?);
                    }
                    self.expect_sym(")")?;
                    rows.push(row);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                return Ok(Statement::Insert { table, columns, source: InsertSource::Values(rows) });
            }
            let q = self.query()?;
            return Ok(Statement::Insert { table, columns, source: InsertSource::Query(Box::new(q)) });
        }
        if self.is_kw("begin") || self.is_kw("commit") || self.is_kw("analyze") || self.is_kw("pragma") || self.is_kw("set") {
            self.skip_to_end();
            return Ok(Statement::NoOp);
        }
        Err(self.err("unsupported statement"))
    }

    fn ident_any(&mut self) -> Result<String, String> {
        match self.next() {
            Tok::Word { value, .. } => Ok(value),
            _ => Err(self.err("expected name")),
        }
    }

    fn skip_to_end(&mut self) {
        while !matches!(self.peek(), Tok::Sym(";") | Tok::Eof) {
            self.next();
        }
    }

    fn skip_parens_until_comma(&mut self) {
        let mut depth = 0;
        loop {
            match self.peek() {
                Tok::Sym("(") => depth += 1,
                Tok::Sym(")") if depth == 0 => return,
                Tok::Sym(")") => depth -= 1,
                Tok::Sym(",") if depth == 0 => return,
                Tok::Eof => return,
                _ => {}
            }
            self.next();
        }
    }

    // ------------------------------------------------------------------ queries

    pub fn query(&mut self) -> Result<Query, String> {
        let mut ctes = Vec::new();
        if self.eat_kw("with") {
            self.eat_kw("recursive");
            loop {
                let name = self.ident()?;
                self.expect_kw("as")?;
                self.expect_sym("(")?;
                let q = self.query()?;
                self.expect_sym(")")?;
                ctes.push((name, q));
                if !self.eat_sym(",") {
                    break;
                }
            }
        }
        let body = self.set_expr(0)?;
        let mut order_by = Vec::new();
        if self.eat_kw("order") {
            self.expect_kw("by")?;
            loop {
                let expr = self.expr()?;
                let asc = if self.eat_kw("desc") {
                    false
                } else {
                    self.eat_kw("asc");
                    true
                };
                let nulls_first = if self.eat_kw("nulls") {
                    if self.eat_kw("first") {
                        Some(true)
                    } else {
                        self.expect_kw("last")?;
                        Some(false)
                    }
                } else {
                    None
                };
                order_by.push(OrderItem { expr, asc, nulls_first });
                if !self.eat_sym(",") {
                    break;
                }
            }
        }
        let mut limit = None;
        let mut offset = None;
        loop {
            if self.eat_kw("limit") {
                let first = self.expr()?;
                if self.eat_sym(",") {
                    // LIMIT offset, count
                    offset = Some(first);
                    limit = Some(self.expr()?);
                } else {
                    limit = Some(first);
                }
            } else if self.eat_kw("offset") {
                offset = Some(self.expr()?);
            } else {
                break;
            }
        }
        Ok(Query { ctes, body, order_by, limit, offset })
    }

    fn set_expr(&mut self, min_prec: u8) -> Result<SetExpr, String> {
        let mut left = if self.eat_sym("(") {
            let q = self.query()?;
            self.expect_sym(")")?;
            SetExpr::Query(Box::new(q))
        } else {
            SetExpr::Select(Box::new(self.select()?))
        };
        loop {
            let (op, prec) = if self.is_kw("union") {
                (SetOp::Union, 1)
            } else if self.is_kw("except") {
                (SetOp::Except, 1)
            } else if self.is_kw("intersect") {
                (SetOp::Intersect, 2)
            } else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.next();
            let all = self.eat_kw("all");
            if !all {
                self.eat_kw("distinct");
            }
            let right = self.set_expr(prec + 1)?;
            left = SetExpr::SetOp { op, all, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn select(&mut self) -> Result<Select, String> {
        self.expect_kw("select")?;
        let distinct = if self.eat_kw("distinct") {
            true
        } else {
            self.eat_kw("all");
            false
        };
        let mut projection = Vec::new();
        loop {
            if self.eat_sym("*") {
                projection.push(SelectItem::Wildcard);
            } else if self.is_ident_start()
                && matches!(self.peek_at(1), Tok::Sym("."))
                && matches!(self.peek_at(2), Tok::Sym("*"))
            {
                let q = self.ident()?;
                self.next();
                self.next();
                projection.push(SelectItem::QualifiedWildcard(q));
            } else {
                let expr = self.expr()?;
                let alias = self.alias()?;
                projection.push(SelectItem::Expr { expr, alias });
            }
            if !self.eat_sym(",") {
                break;
            }
        }
        let mut from = Vec::new();
        if self.eat_kw("from") {
            loop {
                from.push(self.table_ref()?);
                if !self.eat_sym(",") {
                    break;
                }
            }
        }
        let selection = if self.eat_kw("where") { Some(self.expr()?) } else { None };
        let mut group_by = Vec::new();
        if self.eat_kw("group") {
            self.expect_kw("by")?;
            loop {
                group_by.push(self.expr()?);
                if !self.eat_sym(",") {
                    break;
                }
            }
        }
        let having = if self.eat_kw("having") { Some(self.expr()?) } else { None };
        Ok(Select { distinct, projection, from, selection, group_by, having })
    }

    fn alias(&mut self) -> Result<Option<String>, String> {
        if self.eat_kw("as") {
            return Ok(Some(self.ident()?));
        }
        if self.is_ident_start() {
            return Ok(Some(self.ident()?));
        }
        Ok(None)
    }

    fn table_ref(&mut self) -> Result<TableRef, String> {
        let mut left = self.table_factor()?;
        loop {
            let kind = if self.eat_kw("cross") {
                self.expect_kw("join")?;
                JoinKind::Cross
            } else if self.eat_kw("inner") {
                self.expect_kw("join")?;
                JoinKind::Inner
            } else if self.eat_kw("join") {
                JoinKind::Inner
            } else if self.eat_kw("left") {
                self.eat_kw("outer");
                self.expect_kw("join")?;
                JoinKind::Left
            } else if self.eat_kw("right") {
                self.eat_kw("outer");
                self.expect_kw("join")?;
                JoinKind::Right
            } else {
                break;
            };
            let right = self.table_factor()?;
            let mut on = None;
            let mut using = Vec::new();
            if kind != JoinKind::Cross {
                if self.eat_kw("on") {
                    on = Some(self.expr()?);
                } else if self.eat_kw("using") {
                    self.expect_sym("(")?;
                    using.push(self.ident()?);
                    while self.eat_sym(",") {
                        using.push(self.ident()?);
                    }
                    self.expect_sym(")")?;
                } else {
                    return Err(self.err("expected ON or USING"));
                }
            }
            left = TableRef::Join { left: Box::new(left), right: Box::new(right), kind, on, using };
        }
        Ok(left)
    }

    fn table_factor(&mut self) -> Result<TableRef, String> {
        if self.eat_sym("(") {
            if self.is_kw("select") || self.is_kw("with") {
                let q = self.query()?;
                self.expect_sym(")")?;
                let alias = self.alias()?;
                let mut columns = Vec::new();
                if alias.is_some() && self.eat_sym("(") {
                    columns.push(self.ident()?);
                    while self.eat_sym(",") {
                        columns.push(self.ident()?);
                    }
                    self.expect_sym(")")?;
                }
                return Ok(TableRef::Subquery { query: Box::new(q), alias, columns });
            }
            let t = self.table_ref()?;
            self.expect_sym(")")?;
            return Ok(t);
        }
        let name = self.ident()?;
        let alias = self.alias()?;
        Ok(TableRef::Table { name, alias })
    }

    // ------------------------------------------------------------------ expressions

    pub fn expr(&mut self) -> Result<AstExpr, String> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<AstExpr, String> {
        let mut l = self.and_expr()?;
        while self.eat_kw("or") {
            let r = self.and_expr()?;
            l = AstExpr::Binary { op: BinOp::Or, left: Box::new(l), right: Box::new(r) };
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> Result<AstExpr, String> {
        let mut l = self.not_expr()?;
        while self.eat_kw("and") {
            let r = self.not_expr()?;
            l = AstExpr::Binary { op: BinOp::And, left: Box::new(l), right: Box::new(r) };
        }
        Ok(l)
    }

    fn not_expr(&mut self) -> Result<AstExpr, String> {
        if self.is_kw("not") && !self.is_kw_at(1, "exists") {
            self.next();
            return Ok(AstExpr::Not(Box::new(self.not_expr()?)));
        }
        self.cmp_expr()
    }

    fn cmp_expr(&mut self) -> Result<AstExpr, String> {
        let l = self.concat_expr()?;
        // postfix predicates
        let negated = if self.is_kw("not")
            && (self.is_kw_at(1, "in") || self.is_kw_at(1, "between") || self.is_kw_at(1, "like"))
        {
            self.next();
            true
        } else {
            false
        };
        if self.eat_kw("is") {
            let neg = self.eat_kw("not");
            if self.eat_kw("null") {
                return Ok(AstExpr::IsNull { expr: Box::new(l), negated: neg });
            }
            if self.eat_kw("distinct") {
                if !self.eat_kw("from") {
                    return Err(self.err("expected FROM after IS DISTINCT"));
                }
                let r = self.concat_expr()?;
                return Ok(distinct_from(l, r, neg));
            }
            return Err(self.err("expected NULL or DISTINCT FROM after IS"));
        }
        if self.eat_kw("in") {
            self.expect_sym("(")?;
            if self.is_kw("select") || self.is_kw("with") {
                let q = self.query()?;
                self.expect_sym(")")?;
                return Ok(AstExpr::InSubquery { expr: Box::new(l), query: Box::new(q), negated });
            }
            let mut list = Vec::new();
            if !matches!(self.peek(), Tok::Sym(")")) {
                list.push(self.expr()?);
                while self.eat_sym(",") {
                    list.push(self.expr()?);
                }
            }
            self.expect_sym(")")?;
            return Ok(AstExpr::InList { expr: Box::new(l), list, negated });
        }
        if self.eat_kw("between") {
            let low = self.concat_expr()?;
            self.expect_kw("and")?;
            let high = self.concat_expr()?;
            return Ok(AstExpr::Between { expr: Box::new(l), low: Box::new(low), high: Box::new(high), negated });
        }
        if self.eat_kw("like") {
            let p = self.concat_expr()?;
            return Ok(AstExpr::Like { expr: Box::new(l), pattern: Box::new(p), negated });
        }
        if negated {
            return Err(self.err("expected IN, BETWEEN or LIKE after NOT"));
        }
        let op = match self.peek() {
            Tok::Sym("=") => BinOp::Eq,
            Tok::Sym("<>") | Tok::Sym("!=") => BinOp::NotEq,
            Tok::Sym("<") => BinOp::Lt,
            Tok::Sym("<=") => BinOp::LtEq,
            Tok::Sym(">") => BinOp::Gt,
            Tok::Sym(">=") => BinOp::GtEq,
            _ => return Ok(l),
        };
        self.next();
        let r = self.concat_expr()?;
        Ok(AstExpr::Binary { op, left: Box::new(l), right: Box::new(r) })
    }

    fn concat_expr(&mut self) -> Result<AstExpr, String> {
        let mut l = self.add_expr()?;
        while self.eat_sym("||") {
            let r = self.add_expr()?;
            l = AstExpr::Binary { op: BinOp::Concat, left: Box::new(l), right: Box::new(r) };
        }
        Ok(l)
    }

    fn add_expr(&mut self) -> Result<AstExpr, String> {
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Sym("+") => BinOp::Plus,
                Tok::Sym("-") => BinOp::Minus,
                _ => break,
            };
            self.next();
            let r = self.mul_expr()?;
            l = AstExpr::Binary { op, left: Box::new(l), right: Box::new(r) };
        }
        Ok(l)
    }

    fn mul_expr(&mut self) -> Result<AstExpr, String> {
        let mut l = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Sym("*") => BinOp::Mul,
                Tok::Sym("/") => BinOp::Div,
                Tok::Sym("%") => BinOp::Mod,
                _ => break,
            };
            self.next();
            let r = self.unary_expr()?;
            l = AstExpr::Binary { op, left: Box::new(l), right: Box::new(r) };
        }
        Ok(l)
    }

    fn unary_expr(&mut self) -> Result<AstExpr, String> {
        if self.eat_sym("-") {
            let e = self.unary_expr()?;
            return Ok(AstExpr::Neg(Box::new(e)));
        }
        if self.eat_sym("+") {
            return self.unary_expr();
        }
        let mut e = self.primary()?;
        while self.eat_sym("::") {
            let t = self.ident_any()?;
            e = AstExpr::Cast { expr: Box::new(e), dtype: parse_type_name(&t)? };
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<AstExpr, String> {
        match self.peek().clone() {
            Tok::Number(n) => {
                self.next();
                Ok(AstExpr::Lit(Literal::Number(n)))
            }
            Tok::Str(s) => {
                self.next();
                Ok(AstExpr::Lit(Literal::Str(s)))
            }
            Tok::Sym("(") => {
                self.next();
                if self.is_kw("select") || self.is_kw("with") {
                    let q = self.query()?;
                    self.expect_sym(")")?;
                    return Ok(AstExpr::Subquery(Box::new(q)));
                }
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            Tok::Word { value, quoted } => {
                if !quoted {
                    match value.as_str() {
                        "null" => {
                            self.next();
                            return Ok(AstExpr::Lit(Literal::Null));
                        }
                        "true" | "false" => {
                            self.next();
                            return Ok(AstExpr::Lit(Literal::Bool(value == "true")));
                        }
                        "case" => return self.case_expr(),
                        "cast" => {
                            self.next();
                            self.expect_sym("(")?;
                            let e = self.expr()?;
                            self.expect_kw("as")?;
                            let t = self.ident_any()?;
                            let dtype = parse_type_name(&t)?;
                            if self.eat_sym("(") {
                                while !self.eat_sym(")") {
                                    self.next();
                                }
                            }
                            self.expect_sym(")")?;
                            return Ok(AstExpr::Cast { expr: Box::new(e), dtype });
                        }
                        "exists" => {
                            self.next();
                            self.expect_sym("(")?;
                            let q = self.query()?;
                            self.expect_sym(")")?;
                            return Ok(AstExpr::Exists { query: Box::new(q), negated: false });
                        }
                        "not" if self.is_kw_at(1, "exists") => {
                            self.next();
                            self.next();
                            self.expect_sym("(")?;
                            let q = self.query()?;
                            self.expect_sym(")")?;
                            return Ok(AstExpr::Exists { query: Box::new(q), negated: true });
                        }
                        "date" if matches!(self.peek_at(1), Tok::Str(_)) => {
                            self.next();
                            if let Tok::Str(s) = self.next() {
                                return Ok(AstExpr::TypedString { dtype: DataType::Date, value: s });
                            }
                            unreachable!()
                        }
                        "interval" => {
                            self.next();
                            let value = match self.next() {
                                Tok::Str(s) => s,
                                Tok::Number(n) => n,
                                _ => return Err(self.err("expected interval value")),
                            };
                            // INTERVAL '3 months' or INTERVAL '90' DAY
                            let (value, unit) = if let Tok::Word { value: u, quoted: false } = self.peek().clone() {
                                if is_interval_unit(&u) {
                                    self.next();
                                    (value, u)
                                } else {
                                    split_interval(&value)?
                                }
                            } else {
                                split_interval(&value)?
                            };
                            return Ok(AstExpr::Interval { value, unit });
                        }
                        "extract" if matches!(self.peek_at(1), Tok::Sym("(")) => {
                            self.next();
                            self.next();
                            let field = self.ident_any()?;
                            self.expect_kw("from")?;
                            let e = self.expr()?;
                            self.expect_sym(")")?;
                            return Ok(AstExpr::Extract { field, expr: Box::new(e) });
                        }
                        "substring" | "substr" if matches!(self.peek_at(1), Tok::Sym("(")) => {
                            self.next();
                            self.next();
                            let e = self.expr()?;
                            let mut args = vec![e];
                            if self.eat_kw("from") {
                                args.push(self.expr()?);
                                if self.eat_kw("for") {
                                    args.push(self.expr()?);
                                }
                            } else {
                                while self.eat_sym(",") {
                                    args.push(self.expr()?);
                                }
                            }
                            self.expect_sym(")")?;
                            return Ok(AstExpr::Func { name: "substring".into(), args, distinct: false, star: false });
                        }
                        _ => {}
                    }
                    if RESERVED.contains(&value.as_str()) {
                        return Err(self.err("unexpected keyword"));
                    }
                }
                self.next();
                if matches!(self.peek(), Tok::Sym("(")) {
                    self.next();
                    if self.eat_sym("*") {
                        self.expect_sym(")")?;
                        return Ok(AstExpr::Func { name: value, args: vec![], distinct: false, star: true });
                    }
                    let distinct = self.eat_kw("distinct");
                    if !distinct {
                        self.eat_kw("all");
                    }
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Tok::Sym(")")) {
                        args.push(self.expr()?);
                        while self.eat_sym(",") {
                            args.push(self.expr()?);
                        }
                    }
                    self.expect_sym(")")?;
                    return Ok(AstExpr::Func { name: value, args, distinct, star: false });
                }
                let mut parts = vec![value];
                while matches!(self.peek(), Tok::Sym(".")) && matches!(self.peek_at(1), Tok::Word { .. }) {
                    self.next();
                    if let Tok::Word { value, .. } = self.next() {
                        parts.push(value);
                    }
                }
                Ok(AstExpr::Ident(parts))
            }
            _ => Err(self.err("expected expression")),
        }
    }

    fn case_expr(&mut self) -> Result<AstExpr, String> {
        self.expect_kw("case")?;
        let operand = if self.is_kw("when") { None } else { Some(Box::new(self.expr()?)) };
        let mut whens = Vec::new();
        while self.eat_kw("when") {
            let c = self.expr()?;
            self.expect_kw("then")?;
            let r = self.expr()?;
            whens.push((c, r));
        }
        if whens.is_empty() {
            return Err(self.err("expected WHEN"));
        }
        let else_ = if self.eat_kw("else") { Some(Box::new(self.expr()?)) } else { None };
        self.expect_kw("end")?;
        Ok(AstExpr::Case { operand, whens, else_ })
    }
}

fn is_interval_unit(u: &str) -> bool {
    matches!(u, "day" | "days" | "month" | "months" | "year" | "years" | "week" | "weeks")
}

fn split_interval(s: &str) -> Result<(String, String), String> {
    let mut it = s.split_whitespace();
    let v = it.next().ok_or("empty interval")?.to_string();
    let u = it.next().unwrap_or("day").to_ascii_lowercase();
    if !is_interval_unit(&u) {
        return Err(format!("unsupported interval unit {}", u));
    }
    Ok((v, u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_tpch_shapes() {
        let q = "SELECT l_returnflag, sum(l_quantity) AS s FROM lineitem WHERE l_shipdate <= CAST('1998-09-02' AS date) \
                 AND l_discount BETWEEN 0.05 AND 0.07 GROUP BY l_returnflag ORDER BY 1 DESC LIMIT 10";
        let s = parse_statement(q).unwrap();
        match s {
            Statement::Query(q) => {
                assert_eq!(q.order_by.len(), 1);
                assert!(!q.order_by[0].asc);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn precedence() {
        let s = parse_statement("SELECT 1 + 2 * 3 = 7 AND NOT a OR b").unwrap();
        let Statement::Query(q) = s else { panic!() };
        let SetExpr::Select(sel) = q.body else { panic!() };
        let SelectItem::Expr { expr, .. } = &sel.projection[0] else { panic!() };
        assert!(matches!(expr, AstExpr::Binary { op: BinOp::Or, .. }));
    }

    #[test]
    fn derived_table_with_column_aliases() {
        let s = parse_statement(
            "SELECT c_count FROM (SELECT c_custkey, count(o_orderkey) FROM customer LEFT OUTER JOIN orders \
             ON c_custkey = o_custkey AND o_comment NOT LIKE '%special%' GROUP BY c_custkey) AS c_orders (c_custkey, c_count)",
        )
        .unwrap();
        let Statement::Query(q) = s else { panic!() };
        let SetExpr::Select(sel) = q.body else { panic!() };
        let TableRef::Subquery { columns, .. } = &sel.from[0] else { panic!() };
        assert_eq!(columns, &vec!["c_custkey".to_string(), "c_count".to_string()]);
    }

    #[test]
    fn interval_forms() {
        for q in ["SELECT date '1998-12-01' - interval '90' day", "SELECT date '1998-12-01' - interval '3 months'"] {
            parse_statement(q).unwrap();
        }
    }

    #[test]
    fn reports_position() {
        let e = parse_statement("SELECT a FROM WHERE").unwrap_err();
        assert!(e.contains("position"), "{}", e);
    }
}
