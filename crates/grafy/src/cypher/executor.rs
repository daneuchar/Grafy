//! Cypher-Lite executor.
//!
//! Read-only. Opens a single `ReadTransaction` for the duration of each query.
//! No `begin_write` anywhere in this module.

use std::collections::HashMap;

use redb::ReadableTable;
use tracing::{debug, warn};

use crate::cypher::ast::{BinOp, Expr, Lit, ReturnExpr, UnaryOp};
use crate::cypher::error::CypherError;
use crate::cypher::plan::{kind_name, EdgePat, Pipeline, PlanNode};
use crate::store::{EdgeKind, NodeRecord, EDGES_TABLE, NODES_TABLE};

/// Maximum rows before we abort with a hard cap error.
pub const MAX_ROWS: usize = 100_000;

// ---------------------------------------------------------------------------
// Value and Row types
// ---------------------------------------------------------------------------

/// A single column value in a result row.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// An entire node record bound to a variable.
    Node(NodeValue),
    /// An edge bound to a variable.
    Edge(EdgeValue),
}

/// Serialisable node record carried in a row.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NodeValue {
    pub id: u64,
    pub fqn: String,
    pub kind: String,
    pub file: String,
    pub byte_start: u32,
    pub byte_end: u32,
}

/// Serialisable edge carried in a row.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EdgeValue {
    pub from: u64,
    pub to: u64,
    pub kind: String,
}

/// A result row: variable-name → value.
pub type Row = HashMap<String, Value>;

// ---------------------------------------------------------------------------
// Internal execution row (before projection)
// ---------------------------------------------------------------------------

/// An internal row tracking all bound variables.
type InternalRow = HashMap<String, Value>;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Execute a `Pipeline` against `db`. Read-only.
pub fn run(db: &redb::Database, pipeline: &Pipeline) -> Result<Vec<Row>, CypherError> {
    let tx = db
        .begin_read()
        .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

    let nodes_tbl = tx
        .open_table(NODES_TABLE)
        .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

    let edges_tbl = tx
        .open_table(EDGES_TABLE)
        .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

    let mut rows: Vec<InternalRow> = vec![HashMap::new()];

    for node in pipeline {
        match node {
            PlanNode::Scan { var, label, props } => {
                debug!(target: "grafy.cypher.executor", var = %var, ?label, "Scan");
                let mut out = Vec::new();
                for entry in nodes_tbl
                    .iter()
                    .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?
                {
                    let (id_guard, bytes_guard) =
                        entry.map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;
                    let id = id_guard.value();
                    let bytes = bytes_guard.value();
                    let record: NodeRecord = postcard::from_bytes(bytes)
                        .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

                    // Label filter
                    if let Some(k) = label {
                        if record.kind as u8 != *k as u8 {
                            continue;
                        }
                    }

                    // Prop equality filter
                    let nv = node_value(id, &record);
                    if !props_match(&nv, props) {
                        continue;
                    }

                    // Multiply out against existing rows (cross product with head scan)
                    for row in &rows {
                        if out.len() >= MAX_ROWS {
                            warn!(target: "grafy.cypher.executor", "MAX_ROWS cap hit during Scan");
                            return Err(CypherError::Execute {
                                msg: format!(
                                    "query exceeded internal row cap of {MAX_ROWS}; add a LIMIT clause to your query"
                                ),
                            });
                        }
                        let mut r = row.clone();
                        r.insert(var.clone(), Value::Node(nv.clone()));
                        out.push(r);
                    }
                }
                rows = out;
            }

            PlanNode::Expand { from, edge, to } => {
                debug!(target: "grafy.cypher.executor", from = %from, to = %to, "Expand");
                let mut out = Vec::new();
                for row in &rows {
                    let from_id = match row.get(from.as_str()) {
                        Some(Value::Node(nv)) => nv.id,
                        _ => continue,
                    };

                    // Scan edges with from_id as prefix.
                    // EDGES_TABLE key = (from, to, kind_byte)
                    // Range: (from, 0, 0) ..= (from, u64::MAX, u8::MAX)
                    let range_start = (from_id, 0u64, 0u8);
                    let range_end = (from_id, u64::MAX, u8::MAX);
                    let range = range_start..=range_end;

                    let edge_iter = edges_tbl
                        .range(range)
                        .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

                    for entry in edge_iter {
                        let (key_guard, _) =
                            entry.map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;
                        let (ef, et, ek) = key_guard.value();

                        if ef != from_id {
                            break;
                        }

                        // Direction check
                        if !direction_ok(edge, from_id, ef, et) {
                            continue;
                        }

                        // Edge type filter
                        let kind_str = edge_kind_name(ek);
                        if !edge_types_match(edge, kind_str) {
                            continue;
                        }

                        // Fetch the `to` node
                        let to_bytes = match nodes_tbl
                            .get(et)
                            .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?
                        {
                            Some(b) => b,
                            None => {
                                warn!(target: "grafy.cypher.executor", id = et, "dangling edge — to node missing");
                                continue;
                            }
                        };
                        let to_record: NodeRecord = postcard::from_bytes(to_bytes.value())
                            .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

                        let ev = EdgeValue {
                            from: ef,
                            to: et,
                            kind: kind_str.to_string(),
                        };

                        if out.len() >= MAX_ROWS {
                            warn!(target: "grafy.cypher.executor", "MAX_ROWS cap hit during Expand");
                            return Err(CypherError::Execute {
                                msg: format!(
                                    "query exceeded internal row cap of {MAX_ROWS}; add a LIMIT clause to your query"
                                ),
                            });
                        }

                        let mut r = row.clone();
                        r.insert(to.clone(), Value::Node(node_value(et, &to_record)));
                        if let Some(evar) = &edge.var {
                            r.insert(evar.clone(), Value::Edge(ev));
                        }
                        out.push(r);
                    }

                    // Also handle reverse direction (RightToLeft means edge goes to→from)
                    if matches!(edge.direction, crate::cypher::ast::Direction::RightToLeft) {
                        // For RightToLeft we need to find edges where `to` = from_id.
                        // redb EDGES_TABLE is keyed (from, to, kind), so there's no
                        // efficient reverse index. Do a full scan with from_id filter on `to`.
                        // This is acceptable for the v1.0 scope (bounded by MAX_ROWS cap).
                        let all_edges = edges_tbl
                            .iter()
                            .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;
                        for entry in all_edges {
                            let (key_guard, _) =
                                entry.map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;
                            let (ef2, et2, ek2) = key_guard.value();
                            if et2 != from_id {
                                continue;
                            }
                            let kind_str2 = edge_kind_name(ek2);
                            if !edge_types_match(edge, kind_str2) {
                                continue;
                            }

                            let to_bytes = match nodes_tbl
                                .get(ef2)
                                .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?
                            {
                                Some(b) => b,
                                None => continue,
                            };
                            let to_record: NodeRecord = postcard::from_bytes(to_bytes.value())
                                .map_err(|e| CypherError::Storage(anyhow::anyhow!("{e}")))?;

                            if out.len() >= MAX_ROWS {
                                return Err(CypherError::Execute {
                                    msg: format!(
                                        "query exceeded internal row cap of {MAX_ROWS}; add a LIMIT clause"
                                    ),
                                });
                            }

                            let ev = EdgeValue {
                                from: ef2,
                                to: et2,
                                kind: kind_str2.to_string(),
                            };
                            let mut r = row.clone();
                            r.insert(to.clone(), Value::Node(node_value(ef2, &to_record)));
                            if let Some(evar) = &edge.var {
                                r.insert(evar.clone(), Value::Edge(ev));
                            }
                            out.push(r);
                        }
                    }
                }
                rows = out;
            }

            PlanNode::Filter(expr) => {
                rows.retain(|row| eval_expr(expr, row).is_truthy());
            }

            PlanNode::Project(items) => {
                let projected: Vec<Row> = rows
                    .iter()
                    .map(|row| {
                        let mut out = Row::new();
                        for item in items {
                            let col_name = item.alias.clone().unwrap_or_else(|| match &item.expr {
                                ReturnExpr::Var(v) => v.clone(),
                                ReturnExpr::Prop(v, p) => format!("{v}.{p}"),
                            });
                            let val = eval_return_expr(&item.expr, row);
                            out.insert(col_name, val);
                        }
                        out
                    })
                    .collect();
                rows = projected;
            }

            PlanNode::Distinct => {
                // Deduplicate using JSON serialization as key.
                let mut seen = std::collections::HashSet::new();
                rows.retain(|row| {
                    // Build a sorted key string.
                    let mut pairs: Vec<_> = row.iter().collect();
                    pairs.sort_by_key(|(k, _)| k.as_str());
                    let key = pairs
                        .iter()
                        .map(|(k, v)| format!("{k}:{}", value_key(v)))
                        .collect::<Vec<_>>()
                        .join("|");
                    seen.insert(key)
                });
            }

            PlanNode::Sort(order) => {
                rows.sort_by(|a, b| {
                    for item in &order.items {
                        let va = eval_return_expr(&item.expr, a);
                        let vb = eval_return_expr(&item.expr, b);
                        let ord = compare_values(&va, &vb);
                        let ord = if item.descending { ord.reverse() } else { ord };
                        if ord != std::cmp::Ordering::Equal {
                            return ord;
                        }
                    }
                    std::cmp::Ordering::Equal
                });
            }

            PlanNode::Skip(n) => {
                let n = (*n).max(0) as usize;
                if n < rows.len() {
                    rows = rows.split_off(n);
                } else {
                    rows.clear();
                }
            }

            PlanNode::Take(n) => {
                let n = (*n).max(0) as usize;
                rows.truncate(n);
            }
        }
    }

    Ok(rows)
}

// ---------------------------------------------------------------------------
// Expression evaluation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum EvalResult {
    Value(Value),
}

impl EvalResult {
    fn is_truthy(&self) -> bool {
        match self {
            EvalResult::Value(Value::Bool(b)) => *b,
            EvalResult::Value(Value::Null) => false,
            EvalResult::Value(_) => true,
        }
    }

    fn inner(self) -> Value {
        match self {
            EvalResult::Value(v) => v,
        }
    }
}

fn eval_expr(expr: &Expr, row: &InternalRow) -> EvalResult {
    match expr {
        Expr::Lit(l) => EvalResult::Value(lit_to_value(l)),
        Expr::Id(name) => {
            let v = row.get(name.as_str()).cloned().unwrap_or(Value::Null);
            EvalResult::Value(v)
        }
        Expr::Prop(var, prop) => {
            let v = get_prop(row, var, prop);
            EvalResult::Value(v)
        }
        Expr::IsNull(inner, negated) => {
            let v = eval_expr(inner, row).inner();
            let is_null = matches!(v, Value::Null);
            EvalResult::Value(Value::Bool(if *negated { !is_null } else { is_null }))
        }
        Expr::Unary(UnaryOp::Not, inner) => {
            let b = eval_expr(inner, row).is_truthy();
            EvalResult::Value(Value::Bool(!b))
        }
        Expr::Binary(lhs, op, rhs) => match op {
            BinOp::And => {
                let l = eval_expr(lhs, row).is_truthy();
                let r = eval_expr(rhs, row).is_truthy();
                EvalResult::Value(Value::Bool(l && r))
            }
            BinOp::Or => {
                let l = eval_expr(lhs, row).is_truthy();
                let r = eval_expr(rhs, row).is_truthy();
                EvalResult::Value(Value::Bool(l || r))
            }
            _ => {
                let lv = eval_expr(lhs, row).inner();
                let rv = eval_expr(rhs, row).inner();
                EvalResult::Value(Value::Bool(eval_binary_cmp(op, &lv, &rv)))
            }
        },
    }
}

fn eval_binary_cmp(op: &BinOp, lv: &Value, rv: &Value) -> bool {
    match op {
        BinOp::Eq => values_equal(lv, rv),
        BinOp::Neq => !values_equal(lv, rv),
        BinOp::Lt => compare_values(lv, rv) == std::cmp::Ordering::Less,
        BinOp::Gt => compare_values(lv, rv) == std::cmp::Ordering::Greater,
        BinOp::Le => compare_values(lv, rv) != std::cmp::Ordering::Greater,
        BinOp::Ge => compare_values(lv, rv) != std::cmp::Ordering::Less,
        BinOp::Contains => str_predicate(lv, rv, |h, n| h.contains(n)),
        BinOp::StartsWith => str_predicate(lv, rv, |h, n| h.starts_with(n)),
        BinOp::EndsWith => str_predicate(lv, rv, |h, n| h.ends_with(n)),
        BinOp::Regex => {
            // Regex not wired for simplicity; return false.
            // The lexer recognises `=~` but the spec says "Regex" in BinOp.
            // A full regex engine would require a dep; return false for now
            // and document the limitation.
            false
        }
        BinOp::And | BinOp::Or => unreachable!("handled above"),
    }
}

fn str_predicate(lv: &Value, rv: &Value, f: impl Fn(&str, &str) -> bool) -> bool {
    match (lv, rv) {
        (Value::Str(h), Value::Str(n)) => f(h, n),
        _ => false,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
        _ => false,
    }
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64)
            .partial_cmp(y)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Float(x), Value::Int(y)) => x
            .partial_cmp(&(*y as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => std::cmp::Ordering::Equal,
    }
}

// ---------------------------------------------------------------------------
// Property access
// ---------------------------------------------------------------------------

fn get_prop(row: &InternalRow, var: &str, prop: &str) -> Value {
    match row.get(var) {
        Some(Value::Node(nv)) => node_prop(nv, prop),
        Some(Value::Edge(ev)) => edge_prop(ev, prop),
        _ => Value::Null,
    }
}

fn node_prop(nv: &NodeValue, prop: &str) -> Value {
    match prop {
        "fqn" => Value::Str(nv.fqn.clone()),
        "file" => Value::Str(nv.file.clone()),
        "byte_start" => Value::Int(nv.byte_start as i64),
        "byte_end" => Value::Int(nv.byte_end as i64),
        "kind" => Value::Str(nv.kind.clone()),
        "id" => Value::Int(nv.id as i64),
        _ => Value::Null,
    }
}

fn edge_prop(ev: &EdgeValue, prop: &str) -> Value {
    match prop {
        "kind" => Value::Str(ev.kind.clone()),
        "from" => Value::Int(ev.from as i64),
        "to" => Value::Int(ev.to as i64),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Return expression evaluation
// ---------------------------------------------------------------------------

fn eval_return_expr(expr: &ReturnExpr, row: &InternalRow) -> Value {
    match expr {
        ReturnExpr::Var(name) => row.get(name.as_str()).cloned().unwrap_or(Value::Null),
        ReturnExpr::Prop(var, prop) => {
            // First try live node/edge lookup (pre-projection rows).
            let from_node = get_prop(row, var, prop);
            if !matches!(from_node, Value::Null) {
                return from_node;
            }
            // Fallback: look up the projected column name (post-projection rows).
            let projected_key = format!("{var}.{prop}");
            row.get(projected_key.as_str())
                .cloned()
                .unwrap_or(Value::Null)
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn node_value(id: u64, rec: &NodeRecord) -> NodeValue {
    NodeValue {
        id,
        fqn: rec.fqn.clone(),
        kind: kind_name(rec.kind).to_string(),
        file: rec.file.clone(),
        byte_start: rec.byte_start,
        byte_end: rec.byte_end,
    }
}

fn lit_to_value(l: &Lit) -> Value {
    match l {
        Lit::Int(i) => Value::Int(*i),
        Lit::Float(f) => Value::Float(*f),
        Lit::Str(s) => Value::Str(s.clone()),
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Null => Value::Null,
    }
}

fn props_match(nv: &NodeValue, props: &[(String, Lit)]) -> bool {
    for (k, v) in props {
        let actual = node_prop(nv, k);
        let expected = lit_to_value(v);
        if !values_equal(&actual, &expected) {
            return false;
        }
    }
    true
}

fn edge_kind_name(kind_byte: u8) -> &'static str {
    match kind_byte {
        x if x == EdgeKind::Calls as u8 => "CALLS",
        x if x == EdgeKind::Routes as u8 => "ROUTES",
        _ => "UNKNOWN",
    }
}

fn edge_types_match(edge: &EdgePat, kind_str: &str) -> bool {
    if edge.types.is_empty() {
        return true;
    }
    edge.types.iter().any(|t| t.eq_ignore_ascii_case(kind_str))
}

/// For Expand, the direction from the perspective of the `from` node.
/// In the forward scan we always get edges where ef == from_id, so
/// LeftToRight and Undirected both match. RightToLeft is handled separately.
fn direction_ok(edge: &EdgePat, _from_id: u64, _ef: u64, _et: u64) -> bool {
    // For the forward range scan (ef == from_id), LeftToRight and Undirected are valid.
    !matches!(edge.direction, crate::cypher::ast::Direction::RightToLeft)
}

fn value_key(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.clone(),
        Value::Node(nv) => nv.id.to_string(),
        Value::Edge(ev) => format!("{}->{}", ev.from, ev.to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_to_value_roundtrip() {
        assert_eq!(lit_to_value(&Lit::Int(42)), Value::Int(42));
        assert_eq!(
            lit_to_value(&Lit::Str("hi".into())),
            Value::Str("hi".into())
        );
        assert_eq!(lit_to_value(&Lit::Bool(true)), Value::Bool(true));
        assert!(matches!(lit_to_value(&Lit::Null), Value::Null));
    }
}
