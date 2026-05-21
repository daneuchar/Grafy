//! Cypher-Lite AST — plan §5 subset only.

/// Root query node.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// Multiple MATCH clauses are out of scope (§5); Vec allows the validator
    /// to produce the right `Unsupported::MultipleMatch` error.
    pub match_clauses: Vec<MatchClause>,
    pub where_clause: Option<Expr>,
    pub return_clause: ReturnClause,
    pub order_by: Option<OrderBy>,
    pub skip: Option<i64>,
    pub limit: Option<i64>,
    pub distinct: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchClause {
    pub pattern: Pattern,
}

/// A linear pattern: head node followed by zero or more (rel, node) segments.
/// §5 caps at 3 segments (3 chained relationships).
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub head: NodePat,
    pub segments: Vec<(RelPat, NodePat)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodePat {
    pub var: Option<String>,
    pub label: Option<String>,
    pub properties: Vec<(String, Lit)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelPat {
    pub var: Option<String>,
    /// Relationship type labels (the part after `:`). Multiple `|`-separated
    /// types are stored here; executor treats them as OR.
    pub types: Vec<String>,
    pub direction: Direction,
    pub properties: Vec<(String, Lit)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
    Undirected,
}

/// WHERE expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    /// `var.prop` property access
    Prop(String, String),
    /// bare identifier (variable name)
    Id(String),
    Lit(Lit),
    /// `expr IS NULL` (negated=false) / `expr IS NOT NULL` (negated=true)
    IsNull(Box<Expr>, bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    Contains,
    StartsWith,
    EndsWith,
    Regex,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnClause {
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnItem {
    pub expr: ReturnExpr,
    pub alias: Option<String>,
}

/// Expressions allowed in RETURN / ORDER BY. §5: var, prop access, id()
/// function calls are **not** supported — those fall under `Unsupported::Function`.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnExpr {
    /// Return entire variable (node or edge)
    Var(String),
    /// Return `var.prop`
    Prop(String, String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderBy {
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    pub expr: ReturnExpr,
    pub descending: bool,
}

/// Literal values.
#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
}
