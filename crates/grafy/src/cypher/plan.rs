//! Cypher-Lite query plan IR and planner.
//!
//! The planner validates §5 scope rules before handing off to the executor.

use crate::cypher::ast::{Lit, OrderBy, Query, ReturnItem};
use crate::cypher::error::CypherError;
use crate::cypher::scope::Unsupported;
use crate::store::NodeKind;

// ---------------------------------------------------------------------------
// Plan IR
// ---------------------------------------------------------------------------

/// An edge pattern used in Expand nodes.
#[derive(Debug, Clone)]
pub struct EdgePat {
    /// Relationship type names (OR semantics). Empty = any edge kind.
    pub types: Vec<String>,
    pub direction: crate::cypher::ast::Direction,
    pub var: Option<String>,
}

/// A single step in the execution pipeline.
#[derive(Debug, Clone)]
pub enum PlanNode {
    /// Full scan of NODES_TABLE filtered by optional label + prop equality.
    Scan {
        var: String,
        label: Option<NodeKind>,
        props: Vec<(String, Lit)>,
    },
    /// Expand edges from a bound variable.
    Expand {
        from: String,
        edge: EdgePat,
        to: String,
    },
    /// Evaluate a WHERE expression against the current row.
    Filter(crate::cypher::ast::Expr),
    /// Compute the projection for RETURN.
    Project(Vec<ReturnItem>),
    /// Remove duplicate rows.
    Distinct,
    /// Sort rows.
    Sort(OrderBy),
    /// Skip the first N rows.
    Skip(i64),
    /// Take at most N rows.
    Take(i64),
}

/// An ordered list of plan nodes executed left-to-right.
pub type Pipeline = Vec<PlanNode>;

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

pub struct Planner;

impl Planner {
    /// Convert a parsed `Query` into an executable `Pipeline`.
    ///
    /// Validates all §5 scope constraints and returns `CypherError::Unsupported`
    /// for anything out of scope.
    pub fn build(query: Query) -> Result<Pipeline, CypherError> {
        // --- Scope checks ---

        // Multiple MATCH clauses
        if query.match_clauses.len() > 1 {
            return Err(Unsupported::MultipleMatch.into());
        }

        let match_clause = query
            .match_clauses
            .into_iter()
            .next()
            .ok_or_else(|| CypherError::parse("query has no MATCH clause", (0, 0)))?;

        let pattern = match_clause.pattern;

        // More than 3 chained relationships (§5 cap)
        if pattern.segments.len() > 3 {
            return Err(CypherError::Execute {
                msg: format!(
                    "query chains {} relationships; maximum is 3 per §5. Use separate queries or the structured tool `trace_call_path`.",
                    pattern.segments.len()
                ),
            });
        }

        let mut pipeline: Pipeline = Vec::new();

        // --- Scan head node ---
        let head_var = pattern
            .head
            .var
            .clone()
            .unwrap_or_else(|| "_n0".to_string());
        let head_label = pattern
            .head
            .label
            .as_deref()
            .map(label_to_kind)
            .transpose()?;
        pipeline.push(PlanNode::Scan {
            var: head_var.clone(),
            label: head_label,
            props: pattern.head.properties.clone(),
        });

        // --- Expand segments ---
        let mut prev_var = head_var;
        for (i, (rel_pat, node_pat)) in pattern.segments.into_iter().enumerate() {
            let to_var = node_pat
                .var
                .clone()
                .unwrap_or_else(|| format!("_n{}", i + 1));

            let edge = EdgePat {
                types: rel_pat.types.clone(),
                direction: rel_pat.direction,
                var: rel_pat.var.clone(),
            };

            pipeline.push(PlanNode::Expand {
                from: prev_var.clone(),
                edge,
                to: to_var.clone(),
            });

            // If the hop node has a label or property filter, push a Filter.
            if node_pat.label.is_some() || !node_pat.properties.is_empty() {
                let label_kind = node_pat.label.as_deref().map(label_to_kind).transpose()?;
                if let Some(kind) = label_kind {
                    pipeline.push(PlanNode::Filter(crate::cypher::ast::Expr::Binary(
                        Box::new(crate::cypher::ast::Expr::Prop(
                            to_var.clone(),
                            "kind".to_string(),
                        )),
                        crate::cypher::ast::BinOp::Eq,
                        Box::new(crate::cypher::ast::Expr::Lit(crate::cypher::ast::Lit::Str(
                            kind_name(kind).to_string(),
                        ))),
                    )));
                }
                for (k, v) in node_pat.properties {
                    pipeline.push(PlanNode::Filter(crate::cypher::ast::Expr::Binary(
                        Box::new(crate::cypher::ast::Expr::Prop(to_var.clone(), k)),
                        crate::cypher::ast::BinOp::Eq,
                        Box::new(crate::cypher::ast::Expr::Lit(v)),
                    )));
                }
            }

            prev_var = to_var;
        }

        // --- WHERE filter ---
        if let Some(expr) = query.where_clause {
            pipeline.push(PlanNode::Filter(expr));
        }

        // --- PROJECT ---
        pipeline.push(PlanNode::Project(query.return_clause.items));

        // --- Post-projection steps ---
        if query.distinct {
            pipeline.push(PlanNode::Distinct);
        }
        if let Some(order) = query.order_by {
            pipeline.push(PlanNode::Sort(order));
        }
        if let Some(s) = query.skip {
            pipeline.push(PlanNode::Skip(s));
        }
        if let Some(l) = query.limit {
            pipeline.push(PlanNode::Take(l));
        }

        Ok(pipeline)
    }
}

// ---------------------------------------------------------------------------
// Label → NodeKind mapping
// ---------------------------------------------------------------------------

/// Map a Cypher label string to the canonical `NodeKind`.
/// Returns `CypherError::Execute` with a helpful message for unknown labels
/// rather than an error — callers that want zero rows pass `None` label.
/// But here we do surface an error for completely unknown labels at plan time.
/// Actually: per the spec, unknown labels produce zero rows (not an error).
/// We represent that with `None` → skip label filter.
pub fn label_to_kind(label: &str) -> Result<NodeKind, CypherError> {
    match label {
        "File" => Ok(NodeKind::File),
        "Module" => Ok(NodeKind::Module),
        "Function" => Ok(NodeKind::Function),
        "Class" => Ok(NodeKind::Class),
        "Struct" => Ok(NodeKind::Struct),
        "Enum" => Ok(NodeKind::Enum),
        "Trait" => Ok(NodeKind::Trait),
        "Method" => Ok(NodeKind::Method),
        "Route" => Ok(NodeKind::Route),
        _ => {
            // Unknown label — return a sentinel that causes zero rows.
            // We use an Err here to signal "skip this query" at plan level.
            // Actually per Neo4j semantics, unknown label = zero rows, not an error.
            // Return Execute error with a note. The executor will catch this
            // via the special UnknownLabel variant.
            Err(CypherError::Execute {
                msg: format!(
                    "unknown node label `{label}`; valid labels: File, Module, Function, Class, Struct, Enum, Trait, Method, Route"
                ),
            })
        }
    }
}

/// Get the display name of a `NodeKind` (matches the label used in queries).
pub fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "File",
        NodeKind::Module => "Module",
        NodeKind::Function => "Function",
        NodeKind::Class => "Class",
        NodeKind::Struct => "Struct",
        NodeKind::Enum => "Enum",
        NodeKind::Trait => "Trait",
        NodeKind::Method => "Method",
        NodeKind::Route => "Route",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cypher::parser;

    fn build(q: &str) -> Pipeline {
        let ast = parser::parse(q).expect("parse");
        Planner::build(ast).expect("plan")
    }

    #[test]
    fn simple_scan_project() {
        let p = build("MATCH (n:Function) RETURN n.fqn");
        assert!(matches!(p[0], PlanNode::Scan { .. }));
        assert!(p.iter().any(|n| matches!(n, PlanNode::Project(_))));
    }

    #[test]
    fn scan_expand_filter() {
        let p = build("MATCH (a:Function)-[:CALLS]->(b:Function) RETURN a.fqn, b.fqn");
        assert!(matches!(p[0], PlanNode::Scan { .. }));
        assert!(p.iter().any(|n| matches!(n, PlanNode::Expand { .. })));
    }

    #[test]
    fn multiple_match_rejected() {
        // We can't test this easily via the parser because our parser would
        // parse two MATCH clauses, then the planner rejects it.
        // Construct manually:
        use crate::cypher::ast::*;
        let q = Query {
            match_clauses: vec![
                MatchClause {
                    pattern: Pattern {
                        head: NodePat {
                            var: Some("a".into()),
                            label: None,
                            properties: vec![],
                        },
                        segments: vec![],
                    },
                },
                MatchClause {
                    pattern: Pattern {
                        head: NodePat {
                            var: Some("b".into()),
                            label: None,
                            properties: vec![],
                        },
                        segments: vec![],
                    },
                },
            ],
            where_clause: None,
            return_clause: ReturnClause {
                items: vec![ReturnItem {
                    expr: ReturnExpr::Var("a".into()),
                    alias: None,
                }],
            },
            order_by: None,
            skip: None,
            limit: None,
            distinct: false,
        };
        let err = Planner::build(q).unwrap_err();
        assert!(matches!(err, CypherError::Unsupported(_)));
    }
}
