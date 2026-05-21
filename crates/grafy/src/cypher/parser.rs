//! Cypher-Lite recursive-descent parser.
//!
//! Parses the §5 subset only. Unsupported constructs route to
//! `CypherError::Unsupported`; syntax errors carry byte spans.

use tracing::debug;

use crate::cypher::ast::{
    BinOp, Direction, Expr, Lit, MatchClause, NodePat, OrderBy, OrderItem, Pattern, Query,
    RelPat, ReturnClause, ReturnExpr, ReturnItem, UnaryOp,
};
use crate::cypher::error::CypherError;
use crate::cypher::lexer::{tokenize, Spanned, Token};
use crate::cypher::scope::Unsupported;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse a Cypher-Lite query string into an AST.
/// Returns `Err(CypherError)` on syntax or scope violations.
pub fn parse(query: &str) -> Result<Query, CypherError> {
    debug!(target: "grafy.cypher.parser", "parsing query: {:?}", query);
    let tokens = tokenize(query)?;
    let mut p = Parser::new(&tokens, query);
    let q = p.parse_query()?;
    if !p.is_at_end() {
        let span = p.current_span();
        return Err(CypherError::parse(
            format!("unexpected token after query end: {:?}", p.peek_token()),
            span,
        ));
    }
    Ok(q)
}

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

struct Parser<'a> {
    tokens: &'a [Spanned<Token>],
    pos: usize,
    /// Original query string — for error messages.
    _query: &'a str,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Spanned<Token>], query: &'a str) -> Self {
        Self { tokens, pos: 0, _query: query }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Spanned<Token>> {
        self.tokens.get(self.pos)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.peek().map(|s| &s.token)
    }

    fn current_span(&self) -> (usize, usize) {
        self.peek().map(|s| s.span()).unwrap_or((0, 0))
    }

    fn advance(&mut self) -> Option<&Spanned<Token>> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, expected: &Token) -> Result<&Spanned<Token>, CypherError> {
        let span = self.current_span();
        match self.peek_token() {
            Some(t) if t == expected => Ok(self.advance().unwrap()),
            Some(got) => Err(CypherError::parse(
                format!("expected {expected:?}, got {got:?}"),
                span,
            )),
            None => Err(CypherError::parse(
                format!("expected {expected:?}, got end of query"),
                span,
            )),
        }
    }

    /// Like `expect_ident` but also accepts a subset of keywords that commonly
    /// appear as property names or variable names in real queries.
    fn expect_name(&mut self) -> Result<String, CypherError> {
        let span = self.current_span();
        match self.peek_token().cloned() {
            Some(Token::Ident(s)) => {
                self.advance();
                Ok(s)
            }
            Some(Token::Match) => { self.advance(); Ok("match".into()) }
            Some(Token::With) => { self.advance(); Ok("with".into()) }
            Some(Token::In) => { self.advance(); Ok("in".into()) }
            Some(Token::Asc) => { self.advance(); Ok("ASC".into()) }
            Some(Token::Desc) => { self.advance(); Ok("DESC".into()) }
            Some(other) => Err(CypherError::parse(
                format!("expected identifier, got {other:?}"),
                span,
            )),
            None => Err(CypherError::parse("expected identifier, got end of query", span)),
        }
    }

    fn check(&self, t: &Token) -> bool {
        matches!(self.peek_token(), Some(tok) if tok == t)
    }

    fn eat(&mut self, t: &Token) -> bool {
        if self.check(t) {
            self.advance();
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level parsing
// ---------------------------------------------------------------------------

impl<'a> Parser<'a> {
    fn parse_query(&mut self) -> Result<Query, CypherError> {
        // Detect forbidden top-level keywords immediately.
        self.detect_forbidden_top_level()?;

        let mut match_clauses = Vec::new();

        // Must start with MATCH.
        if !self.check(&Token::Match) {
            let span = self.current_span();
            return Err(CypherError::parse("query must start with MATCH", span));
        }

        while self.check(&Token::Match) {
            self.advance(); // consume MATCH
            let pattern = self.parse_pattern()?;
            match_clauses.push(MatchClause { pattern });
        }

        let where_clause = if self.eat(&Token::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        // RETURN
        let span = self.current_span();
        if !self.eat(&Token::Return) {
            return Err(CypherError::parse("expected RETURN clause", span));
        }

        let distinct = self.eat(&Token::Distinct);
        let return_clause = self.parse_return_clause()?;

        let order_by = if self.check(&Token::Order) {
            self.advance(); // ORDER
            let span = self.current_span();
            if !self.eat(&Token::By) {
                return Err(CypherError::parse("expected BY after ORDER", span));
            }
            Some(self.parse_order_by()?)
        } else {
            None
        };

        let skip = if self.eat(&Token::Skip) {
            Some(self.parse_integer_literal()?)
        } else {
            None
        };

        let limit = if self.eat(&Token::Limit) {
            Some(self.parse_integer_literal()?)
        } else {
            None
        };

        // Optional trailing semicolon.
        self.eat(&Token::Semicolon);

        Ok(Query {
            match_clauses,
            where_clause,
            return_clause,
            order_by,
            skip,
            limit,
            distinct,
        })
    }

    /// Reject any forbidden keyword anywhere in the token stream.
    /// `WITH` is special: it's forbidden as a standalone clause but allowed after
    /// `STARTS` or `ENDS` (as part of `STARTS WITH` / `ENDS WITH` predicates).
    fn detect_forbidden_top_level(&mut self) -> Result<(), CypherError> {
        let toks: Vec<&Token> = self.tokens.iter().map(|s| &s.token).collect();
        for (i, tok) in toks.iter().enumerate() {
            match tok {
                Token::With => {
                    // `WITH` after `STARTS` or `ENDS` is a predicate keyword, not a clause.
                    let prev = if i > 0 { toks.get(i - 1) } else { None };
                    let is_predicate_with = matches!(
                        prev,
                        Some(Token::Starts) | Some(Token::Ends)
                    );
                    if !is_predicate_with {
                        return Err(Unsupported::With.into());
                    }
                }
                Token::Unwind => return Err(Unsupported::Unwind.into()),
                Token::Merge => return Err(Unsupported::Merge.into()),
                Token::Create => return Err(Unsupported::Create.into()),
                Token::Delete => return Err(Unsupported::Delete.into()),
                Token::Set => return Err(Unsupported::Set.into()),
                Token::Remove => return Err(Unsupported::Remove.into()),
                Token::Optional => return Err(Unsupported::OptionalMatch.into()),
                _ => {}
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Pattern: (n:Label)-[:REL]->(m:Label)
    // -----------------------------------------------------------------------

    fn parse_pattern(&mut self) -> Result<Pattern, CypherError> {
        // Detect path variable: `p = (a)-...`
        // Looks like: Ident `=` LParen
        if matches!(self.peek_token(), Some(Token::Ident(_))) {
            if let Some(next) = self.tokens.get(self.pos + 1) {
                if next.token == Token::Eq {
                    return Err(Unsupported::PathVariable.into());
                }
            }
        }

        let head = self.parse_node_pat()?;
        let mut segments = Vec::new();

        while self.is_at_rel_start() {
            let rel = self.parse_rel_pat()?;
            let node = self.parse_node_pat()?;
            segments.push((rel, node));
        }

        Ok(Pattern { head, segments })
    }

    /// Returns true if the current position looks like the start of a
    /// relationship pattern.
    fn is_at_rel_start(&self) -> bool {
        matches!(
            self.peek_token(),
            Some(Token::Arrow | Token::LeftArrow | Token::DoubleDash | Token::Minus)
        )
    }

    fn parse_node_pat(&mut self) -> Result<NodePat, CypherError> {
        self.expect(&Token::LParen)?;
        let var = self.try_parse_var();
        let label = if self.eat(&Token::Colon) {
            Some(self.expect_name()?)
        } else {
            None
        };
        let properties = if self.check(&Token::LBrace) {
            self.parse_prop_map()?
        } else {
            Vec::new()
        };
        self.expect(&Token::RParen)?;
        Ok(NodePat { var, label, properties })
    }

    fn try_parse_var(&mut self) -> Option<String> {
        if let Some(Token::Ident(name)) = self.peek_token().cloned() {
            // Check that the next token after the ident is not `=` (path var
            // detection happens earlier) and that it's a valid var context.
            // The next token should be `:`, `)`, `{`, or a relationship punctuation.
            self.advance();
            Some(name)
        } else {
            None
        }
    }

    fn parse_rel_pat(&mut self) -> Result<RelPat, CypherError> {
        // Patterns:
        //   `->`                       LeftToRight (no body)
        //   `<-`                       RightToLeft (no body)
        //   `--`                       Undirected (no body)
        //   `-[...]->`                 LeftToRight with body
        //   `<-[...]-`                 RightToLeft with body
        //   `-[...]-`                  Undirected with body

        let dir_prefix = match self.peek_token() {
            Some(Token::Arrow) => {
                self.advance();
                return Ok(RelPat {
                    var: None,
                    types: Vec::new(),
                    direction: Direction::LeftToRight,
                    properties: Vec::new(),
                });
            }
            Some(Token::LeftArrow) => {
                self.advance();
                if self.check(&Token::LBracket) {
                    Direction::RightToLeft
                } else {
                    return Ok(RelPat {
                        var: None,
                        types: Vec::new(),
                        direction: Direction::RightToLeft,
                        properties: Vec::new(),
                    });
                }
            }
            Some(Token::DoubleDash) => {
                self.advance();
                return Ok(RelPat {
                    var: None,
                    types: Vec::new(),
                    direction: Direction::Undirected,
                    properties: Vec::new(),
                });
            }
            Some(Token::Minus) => {
                self.advance();
                Direction::Undirected // determine at suffix
            }
            _ => {
                let span = self.current_span();
                return Err(CypherError::parse("expected relationship pattern", span));
            }
        };

        let (var, types, props) = if self.check(&Token::LBracket) {
            self.advance(); // `[`

            // Detect variable-length path: `*`
            if self.check(&Token::Star) {
                return Err(Unsupported::VariableLengthPath.into());
            }

            let var = self.try_parse_var();

            // Detect variable-length after var: `r*`
            if self.check(&Token::Star) {
                return Err(Unsupported::VariableLengthPath.into());
            }

            let types = if self.eat(&Token::Colon) {
                self.parse_rel_types()?
            } else {
                Vec::new()
            };

            // Detect variable-length after types: `[:REL*]`
            if self.check(&Token::Star) {
                return Err(Unsupported::VariableLengthPath.into());
            }

            let props = if self.check(&Token::LBrace) {
                self.parse_prop_map()?
            } else {
                Vec::new()
            };

            self.expect(&Token::RBracket)?;
            (var, types, props)
        } else {
            (None, Vec::new(), Vec::new())
        };

        // Determine direction from suffix.
        let direction = match dir_prefix {
            Direction::RightToLeft => {
                self.eat(&Token::Minus);
                Direction::RightToLeft
            }
            Direction::Undirected => {
                if self.eat(&Token::Arrow) {
                    Direction::LeftToRight
                } else {
                    self.eat(&Token::Minus);
                    Direction::Undirected
                }
            }
            Direction::LeftToRight => Direction::LeftToRight,
        };

        Ok(RelPat { var, types, direction, properties: props })
    }

    /// Parse `REL_TYPE1|REL_TYPE2|...`
    fn parse_rel_types(&mut self) -> Result<Vec<String>, CypherError> {
        let mut types = vec![self.expect_name()?];
        while self.eat(&Token::Pipe) {
            self.eat(&Token::Colon);
            types.push(self.expect_name()?);
        }
        Ok(types)
    }

    fn parse_prop_map(&mut self) -> Result<Vec<(String, Lit)>, CypherError> {
        self.expect(&Token::LBrace)?;
        let mut props = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let key = self.expect_name()?;
            self.expect(&Token::Colon)?;
            let val = self.parse_literal()?;
            props.push((key, val));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(props)
    }

    // -----------------------------------------------------------------------
    // Expressions (WHERE)
    // -----------------------------------------------------------------------

    fn parse_expr(&mut self) -> Result<Expr, CypherError> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr, CypherError> {
        let mut lhs = self.parse_and_expr()?;
        while self.eat(&Token::Or) {
            let rhs = self.parse_and_expr()?;
            lhs = Expr::Binary(Box::new(lhs), BinOp::Or, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and_expr(&mut self) -> Result<Expr, CypherError> {
        let mut lhs = self.parse_not_expr()?;
        while self.eat(&Token::And) {
            let rhs = self.parse_not_expr()?;
            lhs = Expr::Binary(Box::new(lhs), BinOp::And, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_not_expr(&mut self) -> Result<Expr, CypherError> {
        if self.eat(&Token::Not) {
            let inner = self.parse_not_expr()?;
            return Ok(Expr::Unary(UnaryOp::Not, Box::new(inner)));
        }
        self.parse_comparison_expr()
    }

    fn parse_comparison_expr(&mut self) -> Result<Expr, CypherError> {
        let lhs = self.parse_primary_expr()?;

        // IS NULL / IS NOT NULL
        if self.eat(&Token::Is) {
            let negated = self.eat(&Token::Not);
            let span = self.current_span();
            if !self.eat(&Token::Null) {
                return Err(CypherError::parse("expected NULL after IS [NOT]", span));
            }
            return Ok(Expr::IsNull(Box::new(lhs), negated));
        }

        // String predicates
        if self.eat(&Token::Contains) {
            let rhs = self.parse_primary_expr()?;
            return Ok(Expr::Binary(Box::new(lhs), BinOp::Contains, Box::new(rhs)));
        }
        if self.eat(&Token::Starts) {
            let span = self.current_span();
            if !self.eat(&Token::With) {
                return Err(CypherError::parse("expected WITH after STARTS", span));
            }
            let rhs = self.parse_primary_expr()?;
            return Ok(Expr::Binary(Box::new(lhs), BinOp::StartsWith, Box::new(rhs)));
        }
        if self.eat(&Token::Ends) {
            let span = self.current_span();
            if !self.eat(&Token::With) {
                return Err(CypherError::parse("expected WITH after ENDS", span));
            }
            let rhs = self.parse_primary_expr()?;
            return Ok(Expr::Binary(Box::new(lhs), BinOp::EndsWith, Box::new(rhs)));
        }

        // IN — unsupported
        if self.check(&Token::In) {
            return Err(Unsupported::Function.into());
        }

        let op = match self.peek_token() {
            Some(Token::Eq) => BinOp::Eq,
            Some(Token::Neq) => BinOp::Neq,
            Some(Token::Lt) => BinOp::Lt,
            Some(Token::Gt) => BinOp::Gt,
            Some(Token::Le) => BinOp::Le,
            Some(Token::Ge) => BinOp::Ge,
            Some(Token::RegexEq) => BinOp::Regex,
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.parse_primary_expr()?;
        Ok(Expr::Binary(Box::new(lhs), op, Box::new(rhs)))
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, CypherError> {
        let span = self.current_span();

        // Parenthesised expression
        if self.eat(&Token::LParen) {
            let inner = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            return Ok(inner);
        }

        // Literals
        if let Some(lit) = self.try_parse_literal() {
            return Ok(Expr::Lit(lit));
        }

        // NOT keyword
        if self.check(&Token::Not) {
            return self.parse_not_expr();
        }

        // Identifier: `var`, `var.prop`, or `func(...)`
        match self.peek_token().cloned() {
            Some(Token::Ident(name)) => {
                self.advance();
                if self.eat(&Token::Dot) {
                    let prop = self.expect_name()?;
                    return Ok(Expr::Prop(name, prop));
                }
                if self.check(&Token::LParen) {
                    return Err(Unsupported::Function.into());
                }
                Ok(Expr::Id(name))
            }
            _ => Err(CypherError::parse(
                format!("expected expression, got {:?}", self.peek_token()),
                span,
            )),
        }
    }

    fn try_parse_literal(&mut self) -> Option<Lit> {
        match self.peek_token().cloned() {
            Some(Token::IntLit(i)) => { self.advance(); Some(Lit::Int(i)) }
            Some(Token::FloatLit(f)) => { self.advance(); Some(Lit::Float(f)) }
            Some(Token::StringLit(s)) => { self.advance(); Some(Lit::Str(s)) }
            Some(Token::True) => { self.advance(); Some(Lit::Bool(true)) }
            Some(Token::False) => { self.advance(); Some(Lit::Bool(false)) }
            Some(Token::Null) => { self.advance(); Some(Lit::Null) }
            _ => None,
        }
    }

    fn parse_literal(&mut self) -> Result<Lit, CypherError> {
        let span = self.current_span();
        self.try_parse_literal().ok_or_else(|| {
            CypherError::parse(
                format!("expected literal value, got {:?}", self.peek_token()),
                span,
            )
        })
    }

    fn parse_integer_literal(&mut self) -> Result<i64, CypherError> {
        let span = self.current_span();
        match self.peek_token().cloned() {
            Some(Token::IntLit(i)) => {
                self.advance();
                Ok(i)
            }
            Some(other) => Err(CypherError::parse(
                format!("expected integer literal, got {other:?}"),
                span,
            )),
            None => Err(CypherError::parse("expected integer literal, got end of query", span)),
        }
    }

    // -----------------------------------------------------------------------
    // RETURN clause
    // -----------------------------------------------------------------------

    fn parse_return_clause(&mut self) -> Result<ReturnClause, CypherError> {
        let mut items = Vec::new();
        items.push(self.parse_return_item()?);
        while self.eat(&Token::Comma) {
            items.push(self.parse_return_item()?);
        }
        Ok(ReturnClause { items })
    }

    fn parse_return_item(&mut self) -> Result<ReturnItem, CypherError> {
        let expr = self.parse_return_expr()?;
        let alias = if self.eat(&Token::As) {
            Some(self.expect_name()?)
        } else {
            None
        };
        Ok(ReturnItem { expr, alias })
    }

    fn parse_return_expr(&mut self) -> Result<ReturnExpr, CypherError> {
        let span = self.current_span();
        match self.peek_token().cloned() {
            Some(Token::Ident(name)) => {
                self.advance();
                if self.eat(&Token::Dot) {
                    let prop = self.expect_name()?;
                    Ok(ReturnExpr::Prop(name, prop))
                } else if self.check(&Token::LParen) {
                    Err(Unsupported::Function.into())
                } else {
                    Ok(ReturnExpr::Var(name))
                }
            }
            Some(other) => Err(CypherError::parse(
                format!("expected return expression, got {other:?}"),
                span,
            )),
            None => Err(CypherError::parse("expected return expression, got end of query", span)),
        }
    }

    // -----------------------------------------------------------------------
    // ORDER BY
    // -----------------------------------------------------------------------

    fn parse_order_by(&mut self) -> Result<OrderBy, CypherError> {
        let mut items = Vec::new();
        items.push(self.parse_order_item()?);
        while self.eat(&Token::Comma) {
            items.push(self.parse_order_item()?);
        }
        Ok(OrderBy { items })
    }

    fn parse_order_item(&mut self) -> Result<OrderItem, CypherError> {
        let expr = self.parse_return_expr()?;
        let descending = if self.eat(&Token::Desc) {
            true
        } else {
            self.eat(&Token::Asc);
            false
        };
        Ok(OrderItem { expr, descending })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(q: &str) -> Query {
        parse(q).unwrap_or_else(|e| panic!("parse failed for {q:?}: {e}"))
    }

    fn parse_err(q: &str) -> CypherError {
        parse(q).expect_err(&format!("expected error for {q:?}"))
    }

    #[test]
    fn simple_match_return() {
        let q = parse_ok("MATCH (n:Function) RETURN n.fqn");
        assert_eq!(q.match_clauses.len(), 1);
        let head = &q.match_clauses[0].pattern.head;
        assert_eq!(head.label.as_deref(), Some("Function"));
        assert_eq!(head.var.as_deref(), Some("n"));
        assert_eq!(q.return_clause.items.len(), 1);
    }

    #[test]
    fn where_and_order_limit() {
        let q = parse_ok(
            r#"MATCH (n:Function) WHERE n.fqn CONTAINS "main" RETURN n.fqn ORDER BY n.fqn LIMIT 10"#,
        );
        assert!(q.where_clause.is_some());
        assert!(q.order_by.is_some());
        assert_eq!(q.limit, Some(10));
    }

    #[test]
    fn relationship_pattern() {
        let q = parse_ok("MATCH (a:Function)-[:CALLS]->(b:Function) RETURN a.fqn, b.fqn");
        let segs = &q.match_clauses[0].pattern.segments;
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].0.types, vec!["CALLS"]);
        assert_eq!(segs[0].0.direction, Direction::LeftToRight);
    }

    #[test]
    fn three_hop_pattern() {
        let q = parse_ok("MATCH (a)-[r]->(b)-[r2]->(c) RETURN a, b, c");
        assert_eq!(q.match_clauses[0].pattern.segments.len(), 2);
    }

    #[test]
    fn distinct_skip_limit() {
        let q = parse_ok(
            r#"MATCH (n:Module) WHERE n.file STARTS WITH "src/" AND NOT n.fqn = "" RETURN DISTINCT n.fqn SKIP 5 LIMIT 5"#,
        );
        assert!(q.distinct);
        assert_eq!(q.skip, Some(5));
        assert_eq!(q.limit, Some(5));
    }

    #[test]
    fn with_is_unsupported() {
        let e = parse_err("MATCH (n) WITH n RETURN n");
        assert!(matches!(e, CypherError::Unsupported(_)));
        let msg = e.to_string();
        assert!(msg.starts_with("ERROR:"), "got: {msg}");
        assert!(msg.contains("docs/cypher-lite.md"), "got: {msg}");
    }

    #[test]
    fn unwind_is_unsupported() {
        let e = parse_err("UNWIND [1,2,3] AS x RETURN x");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn merge_is_unsupported() {
        let e = parse_err("MERGE (n:Function {fqn: 'foo'})");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn create_is_unsupported() {
        let e = parse_err("CREATE (n:Function)");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn delete_is_unsupported() {
        let e = parse_err("MATCH (n) DELETE n");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn set_is_unsupported() {
        let e = parse_err("MATCH (n) SET n.x = 1");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn remove_is_unsupported() {
        let e = parse_err("MATCH (n) REMOVE n.x");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn optional_match_is_unsupported() {
        let e = parse_err("OPTIONAL MATCH (n) RETURN n");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn variable_length_path_is_unsupported() {
        let e = parse_err("MATCH (a)-[*]->(b) RETURN a");
        assert!(matches!(e, CypherError::Unsupported(_)));
        let msg = e.to_string();
        assert!(msg.contains("docs/cypher-lite.md"), "got: {msg}");
    }

    #[test]
    fn path_variable_is_unsupported() {
        let e = parse_err("MATCH p = (a)-[r]->(b) RETURN p");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }

    #[test]
    fn function_call_in_return_is_unsupported() {
        let e = parse_err("MATCH (n) RETURN id(n)");
        assert!(matches!(e, CypherError::Unsupported(_)));
    }
}
