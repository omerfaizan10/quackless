//! Abstract syntax tree produced by the parser.

use crate::types::DataType;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Query(Query),
    Explain(Query),
    CreateTable { name: String, columns: Vec<(String, DataType)>, if_not_exists: bool },
    CreateView { name: String, query: Query },
    DropTable { name: String, if_exists: bool },
    Insert { table: String, columns: Option<Vec<String>>, source: InsertSource },
    /// Statements I accept but ignore (CREATE INDEX, ANALYZE, BEGIN, COMMIT...).
    NoOp,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InsertSource {
    Values(Vec<Vec<AstExpr>>),
    Query(Box<Query>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub ctes: Vec<(String, Query)>,
    pub body: SetExpr,
    pub order_by: Vec<OrderItem>,
    pub limit: Option<AstExpr>,
    pub offset: Option<AstExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SetExpr {
    Select(Box<Select>),
    SetOp { op: SetOp, all: bool, left: Box<SetExpr>, right: Box<SetExpr> },
    Query(Box<Query>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetOp {
    Union,
    Except,
    Intersect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Select {
    pub distinct: bool,
    pub projection: Vec<SelectItem>,
    pub from: Vec<TableRef>,
    pub selection: Option<AstExpr>,
    pub group_by: Vec<AstExpr>,
    pub having: Option<AstExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectItem {
    Expr { expr: AstExpr, alias: Option<String> },
    Wildcard,
    QualifiedWildcard(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    pub expr: AstExpr,
    pub asc: bool,
    pub nulls_first: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Cross,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TableRef {
    Table { name: String, alias: Option<String> },
    Subquery { query: Box<Query>, alias: Option<String>, columns: Vec<String> },
    Join { left: Box<TableRef>, right: Box<TableRef>, kind: JoinKind, on: Option<AstExpr>, using: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Or,
    And,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Plus,
    Minus,
    Mul,
    Div,
    Mod,
    Concat,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Or => "OR",
            BinOp::And => "AND",
            BinOp::Eq => "=",
            BinOp::NotEq => "<>",
            BinOp::Lt => "<",
            BinOp::LtEq => "<=",
            BinOp::Gt => ">",
            BinOp::GtEq => ">=",
            BinOp::Plus => "+",
            BinOp::Minus => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Concat => "||",
        }
    }
    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq)
    }
    /// The comparison with operands swapped (a < b  ==  b > a).
    pub fn flip(self) -> BinOp {
        match self {
            BinOp::Lt => BinOp::Gt,
            BinOp::LtEq => BinOp::GtEq,
            BinOp::Gt => BinOp::Lt,
            BinOp::GtEq => BinOp::LtEq,
            o => o,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(String),
    Str(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstExpr {
    Ident(Vec<String>),
    Lit(Literal),
    TypedString { dtype: DataType, value: String },
    Interval { value: String, unit: String },
    Binary { op: BinOp, left: Box<AstExpr>, right: Box<AstExpr> },
    Neg(Box<AstExpr>),
    Not(Box<AstExpr>),
    Func { name: String, args: Vec<AstExpr>, distinct: bool, star: bool },
    Case { operand: Option<Box<AstExpr>>, whens: Vec<(AstExpr, AstExpr)>, else_: Option<Box<AstExpr>> },
    Cast { expr: Box<AstExpr>, dtype: DataType },
    Extract { field: String, expr: Box<AstExpr> },
    InList { expr: Box<AstExpr>, list: Vec<AstExpr>, negated: bool },
    InSubquery { expr: Box<AstExpr>, query: Box<Query>, negated: bool },
    Exists { query: Box<Query>, negated: bool },
    Subquery(Box<Query>),
    Between { expr: Box<AstExpr>, low: Box<AstExpr>, high: Box<AstExpr>, negated: bool },
    Like { expr: Box<AstExpr>, pattern: Box<AstExpr>, negated: bool },
    IsNull { expr: Box<AstExpr>, negated: bool },
}
